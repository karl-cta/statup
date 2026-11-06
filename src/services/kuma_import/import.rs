//! What importing a Kuma page does: one folder per group, one service per
//! monitor, under the same names, leaving alone what already answers to a
//! name. Running it again adds only what is new.

use std::collections::HashSet;

use super::page::{KumaGroup, KumaPage};
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{JournalKind, Service, ServiceFolder, User, check_folder_name};
use crate::repositories::{
    FolderRepository, NewJournalEntry, ServiceRepository, SettingsRepository,
};
use crate::services::{ServiceService, service_field_error};

const ADDRESS_SETTING: &str = "kuma_import_address";
const INTERNAL_CERT_SETTING: &str = "kuma_import_internal_cert";

/// What importing one group of the page does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPlan {
    /// Its name in Kuma, trimmed.
    pub name: String,
    pub folder: FolderPlan,
    /// In the order of the group.
    pub monitors: Vec<MonitorPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderPlan {
    /// The folder of this name, whatever the case.
    Existing { id: i64, name: String },
    /// A folder created under this name.
    New(String),
    /// The name cannot be a folder's, and why: its services go in none.
    Refused(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorPlan {
    /// Its name in Kuma, trimmed, as its webhook sends it.
    pub name: String,
    pub fate: MonitorFate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorFate {
    /// Created as a service.
    New,
    /// A service already answers to this name, or one created earlier in
    /// the page will.
    Exists,
    /// Statup cannot take this name as it is, and why: shortened, it would
    /// no longer match the reports.
    Refused(&'static str),
}

impl GroupPlan {
    /// How many services importing it creates.
    pub fn new_services(&self) -> usize {
        self.count(|fate| fate == MonitorFate::New)
    }

    /// How many of its monitors meet the test.
    pub fn count(&self, test: impl Fn(MonitorFate) -> bool) -> usize {
        self.monitors
            .iter()
            .filter(|monitor| test(monitor.fate))
            .count()
    }
}

impl MonitorPlan {
    pub fn is_new(&self) -> bool {
        self.fate == MonitorFate::New
    }
}

/// What an import created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Imported {
    pub services: usize,
    pub folders: usize,
}

/// The page last imported, to import it again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LastImport {
    pub address: String,
    pub internal_cert: bool,
}

impl LastImport {
    pub async fn load(pool: &DbPool) -> Result<Self, AppError> {
        Ok(Self {
            address: SettingsRepository::get(pool, ADDRESS_SETTING)
                .await?
                .unwrap_or_default(),
            internal_cert: SettingsRepository::get(pool, INTERNAL_CERT_SETTING)
                .await?
                .as_deref()
                == Some("true"),
        })
    }

    pub async fn save(&self, pool: &DbPool) -> Result<(), AppError> {
        SettingsRepository::set(pool, ADDRESS_SETTING, &self.address).await?;
        let internal_cert = if self.internal_cert { "true" } else { "false" };
        SettingsRepository::set(pool, INTERNAL_CERT_SETTING, internal_cert).await?;
        Ok(())
    }
}

/// The plan of groups of a page, in its order, against what Statup holds.
pub fn plan_import<'a>(
    groups: impl IntoIterator<Item = &'a KumaGroup>,
    services: &[Service],
    folders: &[ServiceFolder],
) -> Vec<GroupPlan> {
    let mut names = Names {
        services,
        planned: HashSet::new(),
    };
    groups
        .into_iter()
        .map(|group| GroupPlan {
            name: group.name.trim().to_string(),
            folder: folder_plan(&group.name, folders),
            monitors: group
                .monitors
                .iter()
                .map(|monitor| names.plan(&monitor.name))
                .collect(),
        })
        .collect()
}

/// A group's folder, found by name the way two folders may not share one.
fn folder_plan(raw: &str, folders: &[ServiceFolder]) -> FolderPlan {
    let name = match check_folder_name(raw) {
        Ok(name) => name,
        Err(key) => return FolderPlan::Refused(key),
    };
    let wanted = name.to_lowercase();
    folders
        .iter()
        .find(|folder| folder.name.to_lowercase() == wanted)
        .map_or(FolderPlan::New(name), |folder| FolderPlan::Existing {
            id: folder.id,
            name: folder.name.clone(),
        })
}

/// The names services answer to: those of Statup, and those of the
/// services the plan creates, by name and by slug.
struct Names<'a> {
    services: &'a [Service],
    planned: HashSet<String>,
}

impl Names<'_> {
    fn plan(&mut self, raw: &str) -> MonitorPlan {
        let name = raw.trim().to_string();
        let fate = self.fate_of(&name);
        if fate == MonitorFate::New {
            self.planned.insert(name.to_lowercase());
            self.planned.insert(slug::slugify(&name));
        }
        MonitorPlan { name, fate }
    }

    fn fate_of(&self, name: &str) -> MonitorFate {
        if let Some(key) = service_field_error(name, None) {
            return MonitorFate::Refused(key);
        }
        let taken = self.planned.contains(&name.to_lowercase())
            || self.services.iter().any(|service| service.answers_to(name));
        if taken {
            MonitorFate::Exists
        } else {
            MonitorFate::New
        }
    }
}

/// Imports the groups of the page whose name is ticked, planned again
/// against what Statup holds now, so a monitor two groups share goes in
/// the first one ticked. Each step stands on its own: an import cut short
/// is finished by running it again.
pub async fn import_groups(
    pool: &DbPool,
    page: &KumaPage,
    ticked: &[String],
    user: &User,
) -> Result<Imported, AppError> {
    let chosen = page
        .groups
        .iter()
        .filter(|group| ticked.iter().any(|name| name == group.name.trim()));
    let services = ServiceRepository::list_all(pool).await?;
    let folders = FolderRepository::list(pool).await?;
    let mut run = Run {
        pool,
        user,
        imported: Imported::default(),
    };
    for group in plan_import(chosen, &services, &folders) {
        run.group(&group).await?;
    }
    Ok(run.imported)
}

/// One import under way, and what it created so far.
struct Run<'a> {
    pool: &'a DbPool,
    user: &'a User,
    imported: Imported,
}

impl Run<'_> {
    async fn group(&mut self, group: &GroupPlan) -> Result<(), AppError> {
        let new: Vec<&str> = group
            .monitors
            .iter()
            .filter(|monitor| monitor.is_new())
            .map(|monitor| monitor.name.as_str())
            .collect();
        if new.is_empty() {
            return Ok(());
        }
        let folder = self.folder(&group.folder).await?;
        for name in new {
            self.service(name, folder.as_ref()).await?;
        }
        Ok(())
    }

    /// Creates one service, inside its folder from the start: it never
    /// shows with another visibility, even when the import stops there.
    async fn service(
        &mut self,
        name: &str,
        folder: Option<&(i64, String)>,
    ) -> Result<(), AppError> {
        match folder {
            Some((id, _)) => {
                ServiceService::create_in_folder(self.pool, *id, name).await?;
            }
            None => {
                ServiceService::create(self.pool, name, None, None, None).await?;
            }
        }
        NewJournalEntry::new(JournalKind::ServiceCreated)
            .by(self.user)
            .about(name)
            .record(self.pool)
            .await;
        if let Some((_, folder)) = folder {
            NewJournalEntry::new(JournalKind::FolderServiceAdded)
                .by(self.user)
                .about(name)
                .with_detail(folder)
                .record(self.pool)
                .await;
        }
        self.imported.services += 1;
        Ok(())
    }

    /// The folder a group's services go in: found, or created. Groups of
    /// one name are merged when the page is read, so a name is created once.
    async fn folder(&mut self, plan: &FolderPlan) -> Result<Option<(i64, String)>, AppError> {
        let name = match plan {
            FolderPlan::Refused(_) => return Ok(None),
            FolderPlan::Existing { id, name } => return Ok(Some((*id, name.clone()))),
            FolderPlan::New(name) => name,
        };
        let id = FolderRepository::create(self.pool, name).await?;
        NewJournalEntry::new(JournalKind::FolderCreated)
            .by(self.user)
            .about(name)
            .record(self.pool)
            .await;
        self.imported.folders += 1;
        Ok(Some((id, name.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::MonitorFate::{Exists, New, Refused};
    use super::*;
    use crate::models::{Grants, Role, Visibility};
    use crate::repositories::UserRepository;
    use crate::services::kuma_import::page::KumaMonitor;
    use crate::test_helpers::test_pool;

    fn page(groups: &[(&str, &[&str])]) -> KumaPage {
        KumaPage {
            title: Some("Infra".to_string()),
            groups: groups
                .iter()
                .map(|(name, monitors)| KumaGroup {
                    name: (*name).to_string(),
                    monitors: monitors
                        .iter()
                        .map(|monitor| KumaMonitor {
                            name: (*monitor).to_string(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn fates(plans: &[GroupPlan]) -> Vec<Vec<(&str, MonitorFate)>> {
        plans
            .iter()
            .map(|group| {
                group
                    .monitors
                    .iter()
                    .map(|monitor| (monitor.name.as_str(), monitor.fate))
                    .collect()
            })
            .collect()
    }

    async fn editor(pool: &DbPool) -> User {
        UserRepository::create(pool, "editor@example.com", "hash", "Eve", Role::Publisher)
            .await
            .unwrap()
    }

    /// Name, folder name and whether it follows its folder, in the order of
    /// the Services page.
    async fn placed(pool: &DbPool) -> Vec<(String, Option<String>, bool)> {
        let folders = FolderRepository::names(pool).await.unwrap();
        ServiceRepository::list_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|service| {
                let folder = service.folder_id.and_then(|id| folders.get(&id).cloned());
                (service.name, folder, service.follows_folder)
            })
            .collect()
    }

    #[tokio::test]
    async fn a_monitor_is_new_unless_a_service_answers_to_its_name() {
        let pool = test_pool().await;
        ServiceService::create(&pool, "Mail", None, None, None)
            .await
            .unwrap();
        ServiceRepository::create(&pool, "Portail", "intranet", None, None, None)
            .await
            .unwrap();
        let services = ServiceRepository::list_all(&pool).await.unwrap();
        let long = "a".repeat(101);
        let monitors = [
            "mail",
            "Intranet",
            "VPN",
            " VPN ",
            long.as_str(),
            "Wiki Paie",
            "wiki-paie",
        ];
        let plans = plan_import(&page(&[("Infra", &monitors)]).groups, &services, &[]);
        assert_eq!(
            fates(&plans),
            [vec![
                ("mail", Exists),
                ("Intranet", Exists),
                ("VPN", New),
                ("VPN", Exists),
                (long.as_str(), Refused("validation.service_name_too_long")),
                ("Wiki Paie", New),
                ("wiki-paie", Exists),
            ]]
        );
        assert_eq!(plans[0].new_services(), 2);
    }

    #[test]
    fn a_group_goes_in_the_folder_of_its_name() {
        let folders = [ServiceFolder {
            id: 4,
            name: "Réseau".to_string(),
            position: 1,
            visibility: Visibility::Everyone,
        }];
        let plans = plan_import(
            &page(&[(" RÉSEAU ", &[]), ("Infrastructure", &[]), ("  ", &[])]).groups,
            &[],
            &folders,
        );
        let folders: Vec<&FolderPlan> = plans.iter().map(|group| &group.folder).collect();
        assert_eq!(
            folders,
            [
                &FolderPlan::Existing {
                    id: 4,
                    name: "Réseau".to_string()
                },
                &FolderPlan::New("Infrastructure".to_string()),
                &FolderPlan::Refused("validation.folder_name_required"),
            ]
        );
        assert_eq!(plans[0].name, "RÉSEAU");
    }

    #[tokio::test]
    async fn an_import_creates_the_ticked_groups_in_order_then_only_what_is_new() {
        let pool = test_pool().await;
        let user = editor(&pool).await;
        let kuma = page(&[
            ("Infrastructure", &["Mail", "VPN"]),
            ("Réseau", &["Wi-Fi"]),
            ("Tests", &["Recette"]),
        ]);
        let ticked = ["Infrastructure".to_string(), "Réseau".to_string()];
        let imported = import_groups(&pool, &kuma, &ticked, &user).await.unwrap();
        assert_eq!(
            imported,
            Imported {
                services: 3,
                folders: 2
            }
        );
        let infrastructure = Some("Infrastructure".to_string());
        let network = Some("Réseau".to_string());
        assert_eq!(
            placed(&pool).await,
            [
                ("Mail".to_string(), infrastructure.clone(), true),
                ("VPN".to_string(), infrastructure, true),
                ("Wi-Fi".to_string(), network, true),
            ]
        );

        let all = ticked
            .into_iter()
            .chain(["Tests".to_string()])
            .collect::<Vec<_>>();
        let again = import_groups(&pool, &kuma, &all, &user).await.unwrap();
        assert_eq!(
            again,
            Imported {
                services: 1,
                folders: 1
            }
        );
        assert_eq!(placed(&pool).await.len(), 4);

        let journal: Vec<(JournalKind, Option<String>)> =
            sqlx::query_as("SELECT kind, subject FROM journal_entries ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let count = |kind| journal.iter().filter(|(k, _)| *k == kind).count();
        assert_eq!(count(JournalKind::ServiceCreated), 4);
        assert_eq!(count(JournalKind::FolderCreated), 3);
        assert_eq!(count(JournalKind::FolderServiceAdded), 4);
    }

    #[tokio::test]
    async fn a_service_imported_in_an_existing_folder_takes_its_visibility() {
        let pool = test_pool().await;
        let user = editor(&pool).await;
        let folder = FolderRepository::create(&pool, "Paie").await.unwrap();
        FolderRepository::save_settings(
            &pool,
            folder,
            "Paie",
            Visibility::Members,
            Grants::default(),
        )
        .await
        .unwrap();
        let kuma = page(&[("paie", &["Bulletins"])]);
        let imported = import_groups(&pool, &kuma, &["paie".to_string()], &user)
            .await
            .unwrap();
        assert_eq!(
            imported,
            Imported {
                services: 1,
                folders: 0
            }
        );
        let service = &ServiceRepository::list_all(&pool).await.unwrap()[0];
        assert_eq!(service.folder_id, Some(folder));
        assert!(service.follows_folder);
        assert_eq!(service.visibility, Visibility::Members);
    }

    #[tokio::test]
    async fn a_monitor_two_groups_share_goes_in_the_first_one_ticked() {
        let kuma = page(&[("A", &["Mail"]), ("B", &["Mail", "DNS"])]);
        for (ticked, expected) in [
            (vec!["B"], vec![("Mail", "B"), ("DNS", "B")]),
            (vec!["A", "B"], vec![("Mail", "A"), ("DNS", "B")]),
        ] {
            let pool = test_pool().await;
            let user = editor(&pool).await;
            let ticked: Vec<String> = ticked.into_iter().map(String::from).collect();
            import_groups(&pool, &kuma, &ticked, &user).await.unwrap();
            let found: Vec<(String, Option<String>)> = placed(&pool)
                .await
                .into_iter()
                .map(|(name, folder, _)| (name, folder))
                .collect();
            let expected: Vec<(String, Option<String>)> = expected
                .into_iter()
                .map(|(name, folder)| (name.to_string(), Some(folder.to_string())))
                .collect();
            assert_eq!(found, expected, "{ticked:?}");
        }
    }

    #[tokio::test]
    async fn the_last_import_is_remembered() {
        let pool = test_pool().await;
        assert_eq!(
            LastImport::load(&pool).await.unwrap(),
            LastImport::default()
        );
        let last = LastImport {
            address: "https://kuma.example.com/status/infra".to_string(),
            internal_cert: true,
        };
        last.save(&pool).await.unwrap();
        assert_eq!(LastImport::load(&pool).await.unwrap(), last);
    }
}

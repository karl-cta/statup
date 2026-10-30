//! The journal: what members did and what the app met, kept for the
//! administrators.

use chrono::{DateTime, Utc};
use tracing::Level;

use super::Tone;
use crate::i18n::I18n;

/// The family of a line, offered as a filter on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum JournalCategory {
    /// Changes made by a member.
    Members,
    /// Sign-ins and password resets.
    Access,
    /// What the checks of the services found.
    Monitoring,
    /// Messages to the alert channels.
    Alerts,
    /// Errors and warnings of the app itself.
    System,
}

/// What a line records, which decides its family, whether the default view
/// shows it, and its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum JournalKind {
    /// An error the app logged.
    AppError,
    /// A warning the app logged.
    AppWarning,
    /// An automatic backup was written.
    BackupWritten,
    /// An automatic backup could not be written.
    BackupFailed,
    /// A member added a service.
    ServiceCreated,
    /// A member changed a service.
    ServiceUpdated,
    /// A member deleted a service.
    ServiceDeleted,
    /// A member changed who sees a service.
    ServiceVisibilityChanged,
    /// A member set the status of a service.
    ServiceStatusSet,
    /// A member changed the order of the services.
    ServicesReordered,
    /// A member turned the problems-first order on or off.
    ProblemsFirstChanged,
    /// A member saved the general settings.
    SettingsSaved,
    /// A member removed the logo.
    LogoRemoved,
    /// A member changed what the journal keeps.
    JournalSettingsSaved,
    /// A member changed when automatic backups are taken.
    BackupSettingsSaved,
    /// A member downloaded a backup.
    BackupDownloaded,
    /// A backup was restored on an empty instance, from its first screen.
    BackupRestored,
    /// A member changed the address of the page used in alerts.
    PageAddressSaved,
    /// A member added an alert channel.
    ChannelCreated,
    /// A member changed an alert channel.
    ChannelUpdated,
    /// A member deleted an alert channel.
    ChannelDeleted,
    /// A member tried an alert channel.
    ChannelTested,
    /// An administrator made an API key.
    ApiKeyCreated,
    /// An administrator gave an API key a new secret.
    ApiKeyRegenerated,
    /// An administrator revoked an API key.
    ApiKeyRevoked,
    /// A member added someone to the team.
    MemberAdded,
    /// A member changed the role of someone.
    MemberRoleChanged,
    /// A member disabled an account.
    MemberDisabled,
    /// A member enabled an account again.
    MemberEnabled,
    /// A member gave someone a temporary password.
    MemberPasswordReset,
    /// A member created a group of members.
    GroupCreated,
    /// A member renamed a group.
    GroupRenamed,
    /// A member deleted a group.
    GroupDeleted,
    /// A member put someone in a group.
    GroupMemberAdded,
    /// A member took someone out of a group.
    GroupMemberRemoved,
    /// A member created a folder of services.
    FolderCreated,
    /// A member renamed a folder.
    FolderRenamed,
    /// A member deleted a folder; its services stay.
    FolderDeleted,
    /// A member changed who sees a folder.
    FolderVisibilityChanged,
    /// A member put a service in a folder.
    FolderServiceAdded,
    /// A member took a service out of a folder.
    FolderServiceRemoved,
    /// A member published an event.
    EventCreated,
    /// A member edited an event.
    EventEdited,
    /// A member posted news on an event.
    EventUpdatePosted,
    /// A member removed an update of an event.
    EventUpdateDeleted,
    /// A member reopened an event.
    EventReverted,
    /// A member deleted an event.
    EventDeleted,
    /// A member stopped a maintenance from repeating.
    RepeatStopped,
    /// A member carried an edit of a repeating maintenance to the next ones.
    RepeatChanged,
    /// A member published the review of an incident.
    ReviewPublished,
    /// A member removed the review of an incident.
    ReviewRemoved,
    /// A member saved an event template.
    TemplateSaved,
    /// A member deleted an event template.
    TemplateDeleted,
    /// A member added an icon.
    IconUploaded,
    /// A member deleted an icon.
    IconDeleted,
    /// A member arranged the dashboard.
    DashboardArranged,
    /// A member changed their profile.
    ProfileUpdated,
    /// A member changed their own password.
    OwnPasswordChanged,
    /// Someone signed in.
    SignedIn,
    /// A sign-in was refused.
    SignInFailed,
    /// Too many failed sign-ins from one address.
    SignInBlocked,
    /// Too many wrong current passwords for one account.
    PasswordCheckBlocked,
    /// Someone asked for a link to choose a new password.
    PasswordResetRequested,
    /// Too many requests for a link from one address, or for one account.
    PasswordResetBlocked,
    /// The email with the link to a new password could not be sent.
    PasswordResetNotSent,
    /// Someone chose a new password through the emailed link.
    PasswordResetCompleted,
    /// The checks declared a service down.
    OutageDetected,
    /// A service declared down answers again.
    OutageEnded,
    /// A member erased a detected outage that was not one.
    OutageErased,
    /// One check of a service failed.
    CheckFailed,
    /// Every service failed at once: the round was ignored.
    MonitoringRoundSkipped,
    /// Every check failed because Statup is off the network: checks paused.
    NetworkLost,
    /// Statup reaches the network again: checks resumed.
    NetworkBack,
    /// A monitoring tool reported on a service Statup cannot act on.
    ReportIgnored,
    /// A message left for an alert channel.
    AlertSent,
    /// A message did not leave and will be tried again.
    AlertRetrying,
    /// A message was given up.
    AlertAbandoned,
    /// Emails to subscribers given up in one round, for one reason.
    SubscriberMailsAbandoned,
    /// The mail server failed as a whole: emails to subscribers wait for it.
    SubscriberMailsPaused,
    /// A visitor confirmed their subscription by email, or a member subscribed.
    SubscriptionConfirmed,
    /// A subscriber left, by the link of an email or from the page.
    SubscriptionEnded,
    /// An administrator removed a subscriber.
    SubscriberRemoved,
    /// An administrator opened or closed subscriptions to visitors.
    VisitorSubscriptionsChanged,
}

/// Everything a kind says about itself: one line per kind in
/// [`JournalKind::spec`].
#[derive(Debug, Clone, Copy)]
pub struct KindSpec {
    pub category: JournalCategory,
    /// The default view keeps the large changes and the real problems.
    pub essential: bool,
    /// The level of its line in the server output.
    pub level: Level,
    /// The hue of its chip: color only for problems.
    pub tone: Tone,
    /// The line in the server output, in English.
    pub summary: &'static str,
    /// The translation key of its sentence on the page.
    pub key: &'static str,
}

const fn row(
    category: JournalCategory,
    essential: bool,
    level: Level,
    tone: Tone,
    summary: &'static str,
    key: &'static str,
) -> KindSpec {
    KindSpec {
        category,
        essential,
        level,
        tone,
        summary,
        key,
    }
}

impl JournalKind {
    // One line per kind, kept unwrapped so the table reads as a table.
    #[rustfmt::skip]
    pub const fn spec(self) -> KindSpec {
        use JournalCategory as Cat;
        match self {
            Self::AppError => row(Cat::System, true, Level::ERROR, Tone::Crit, "Application error", "journal.app_error"),
            Self::AppWarning => row(Cat::System, false, Level::WARN, Tone::Minor, "Application warning", "journal.app_warning"),
            Self::BackupWritten => row(Cat::System, false, Level::INFO, Tone::Neutral, "Backup written", "journal.backup_written"),
            Self::BackupFailed => row(Cat::System, true, Level::ERROR, Tone::Crit, "Backup failed", "journal.backup_failed"),
            Self::ServiceCreated => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Service created", "journal.service_created"),
            Self::ServiceUpdated => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Service updated", "journal.service_updated"),
            Self::ServiceDeleted => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Service deleted", "journal.service_deleted"),
            Self::ServiceVisibilityChanged => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Service visibility changed", "journal.service_visibility_changed"),
            Self::ServiceStatusSet => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Service status set", "journal.service_status_set"),
            Self::ServicesReordered => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Services reordered", "journal.services_reordered"),
            Self::ProblemsFirstChanged => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Problems first changed", "journal.problems_first_changed"),
            Self::SettingsSaved => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Settings saved", "journal.settings_saved"),
            Self::LogoRemoved => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Logo removed", "journal.logo_removed"),
            Self::JournalSettingsSaved => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Journal settings saved", "journal.journal_settings_saved"),
            Self::BackupSettingsSaved => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Backup settings saved", "journal.backup_settings_saved"),
            Self::BackupRestored => row(Cat::System, true, Level::INFO, Tone::Neutral, "Backup restored", "journal.backup_restored"),
            Self::BackupDownloaded => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Backup downloaded", "journal.backup_downloaded"),
            Self::PageAddressSaved => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Page address saved", "journal.page_address_saved"),
            Self::ChannelCreated => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Alert channel created", "journal.channel_created"),
            Self::ChannelUpdated => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Alert channel updated", "journal.channel_updated"),
            Self::ChannelDeleted => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Alert channel deleted", "journal.channel_deleted"),
            Self::ChannelTested => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Alert channel tested", "journal.channel_tested"),
            Self::ApiKeyCreated => row(Cat::Members, true, Level::INFO, Tone::Neutral, "API key created", "journal.api_key_created"),
            Self::ApiKeyRegenerated => row(Cat::Members, true, Level::INFO, Tone::Neutral, "API key regenerated", "journal.api_key_regenerated"),
            Self::ApiKeyRevoked => row(Cat::Members, true, Level::INFO, Tone::Neutral, "API key revoked", "journal.api_key_revoked"),
            Self::MemberAdded => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Member added", "journal.member_added"),
            Self::MemberRoleChanged => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Member role changed", "journal.member_role_changed"),
            Self::MemberDisabled => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Member disabled", "journal.member_disabled"),
            Self::MemberEnabled => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Member enabled", "journal.member_enabled"),
            Self::MemberPasswordReset => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Member password reset", "journal.member_password_reset"),
            Self::GroupCreated => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Group created", "journal.group_created"),
            Self::GroupRenamed => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Group renamed", "journal.group_renamed"),
            Self::GroupDeleted => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Group deleted", "journal.group_deleted"),
            Self::GroupMemberAdded => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Group member added", "journal.group_member_added"),
            Self::GroupMemberRemoved => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Group member removed", "journal.group_member_removed"),
            Self::FolderCreated => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Folder created", "journal.folder_created"),
            Self::FolderRenamed => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Folder renamed", "journal.folder_renamed"),
            Self::FolderDeleted => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Folder deleted", "journal.folder_deleted"),
            Self::FolderVisibilityChanged => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Folder visibility changed", "journal.folder_visibility_changed"),
            Self::FolderServiceAdded => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Service added to a folder", "journal.folder_service_added"),
            Self::FolderServiceRemoved => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Service removed from a folder", "journal.folder_service_removed"),
            Self::EventCreated => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Event created", "journal.event_created"),
            Self::EventEdited => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Event edited", "journal.event_edited"),
            Self::EventUpdatePosted => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Event update posted", "journal.event_update_posted"),
            Self::EventUpdateDeleted => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Event update deleted", "journal.event_update_deleted"),
            Self::EventReverted => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Event reverted", "journal.event_reverted"),
            Self::EventDeleted => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Event deleted", "journal.event_deleted"),
            Self::RepeatStopped => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Maintenance repeat stopped", "journal.repeat_stopped"),
            Self::RepeatChanged => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Repeating maintenance edited with the next ones", "journal.repeat_changed"),
            Self::ReviewPublished => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Review published", "journal.review_published"),
            Self::ReviewRemoved => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Review removed", "journal.review_removed"),
            Self::TemplateSaved => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Template saved", "journal.template_saved"),
            Self::TemplateDeleted => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Template deleted", "journal.template_deleted"),
            Self::IconUploaded => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Icon uploaded", "journal.icon_uploaded"),
            Self::IconDeleted => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Icon deleted", "journal.icon_deleted"),
            Self::DashboardArranged => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Dashboard arranged", "journal.dashboard_arranged"),
            Self::ProfileUpdated => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Profile updated", "journal.profile_updated"),
            Self::OwnPasswordChanged => row(Cat::Members, false, Level::INFO, Tone::Neutral, "Own password changed", "journal.own_password_changed"),
            Self::SignedIn => row(Cat::Access, false, Level::INFO, Tone::Neutral, "Signed in", "journal.signed_in"),
            Self::SignInFailed => row(Cat::Access, false, Level::WARN, Tone::Neutral, "Sign-in refused", "journal.sign_in_failed"),
            Self::SignInBlocked => row(Cat::Access, true, Level::WARN, Tone::Minor, "Sign-ins blocked", "journal.sign_in_blocked"),
            Self::PasswordCheckBlocked => row(Cat::Access, true, Level::WARN, Tone::Minor, "Password checks blocked", "journal.password_check_blocked"),
            Self::PasswordResetRequested => row(Cat::Access, false, Level::INFO, Tone::Neutral, "Password reset requested", "journal.password_reset_requested"),
            Self::PasswordResetBlocked => row(Cat::Access, true, Level::WARN, Tone::Minor, "Password reset requests blocked", "journal.password_reset_blocked"),
            Self::PasswordResetNotSent => row(Cat::Access, true, Level::WARN, Tone::Minor, "Password reset email not sent", "journal.password_reset_not_sent"),
            Self::PasswordResetCompleted => row(Cat::Access, false, Level::INFO, Tone::Neutral, "Password reset completed", "journal.password_reset_completed"),
            Self::OutageDetected => row(Cat::Monitoring, true, Level::WARN, Tone::Crit, "Outage detected", "journal.outage_detected"),
            Self::OutageErased => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Detected outage erased", "journal.outage_erased"),
            Self::OutageEnded => row(Cat::Monitoring, true, Level::INFO, Tone::Ok, "Service back up", "journal.outage_ended"),
            Self::CheckFailed => row(Cat::Monitoring, false, Level::DEBUG, Tone::Minor, "Check failed", "journal.check_failed"),
            Self::MonitoringRoundSkipped => row(Cat::Monitoring, true, Level::WARN, Tone::Minor, "Monitoring round ignored: every service failed at once", "journal.monitoring_round_skipped"),
            Self::NetworkLost => row(Cat::Monitoring, true, Level::WARN, Tone::Minor, "Statup is off the network: checks paused", "journal.network_lost"),
            Self::NetworkBack => row(Cat::Monitoring, true, Level::INFO, Tone::Ok, "Statup is back on the network: checks resumed", "journal.network_back"),
            Self::ReportIgnored => row(Cat::Monitoring, true, Level::WARN, Tone::Minor, "Report from a monitoring tool ignored", "journal.report_ignored"),
            Self::AlertSent => row(Cat::Alerts, false, Level::DEBUG, Tone::Neutral, "Alert sent", "journal.alert_sent"),
            Self::AlertRetrying => row(Cat::Alerts, false, Level::WARN, Tone::Minor, "Alert not sent, will try again", "journal.alert_retrying"),
            Self::AlertAbandoned => row(Cat::Alerts, true, Level::WARN, Tone::Crit, "Alert given up", "journal.alert_abandoned"),
            Self::SubscriberMailsAbandoned => row(Cat::Alerts, true, Level::WARN, Tone::Crit, "Emails to subscribers given up", "journal.subscriber_mails_abandoned"),
            Self::SubscriberMailsPaused => row(Cat::Alerts, true, Level::WARN, Tone::Minor, "Emails to subscribers on hold: the mail server failed", "journal.subscriber_mails_paused"),
            Self::SubscriptionConfirmed => row(Cat::Alerts, false, Level::INFO, Tone::Neutral, "Email subscription confirmed", "journal.subscription_confirmed"),
            Self::SubscriptionEnded => row(Cat::Alerts, false, Level::INFO, Tone::Neutral, "Email subscription ended", "journal.subscription_ended"),
            Self::SubscriberRemoved => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Email subscriber removed", "journal.subscriber_removed"),
            Self::VisitorSubscriptionsChanged => row(Cat::Members, true, Level::INFO, Tone::Neutral, "Visitor subscriptions changed", "journal.visitor_subscriptions_changed"),
        }
    }

    pub fn category(self) -> JournalCategory {
        self.spec().category
    }

    pub fn is_essential(self) -> bool {
        self.spec().essential
    }

    pub fn level(self) -> Level {
        self.spec().level
    }

    pub fn tone(self) -> Tone {
        self.spec().tone
    }

    pub fn summary(self) -> &'static str {
        self.spec().summary
    }
}

impl JournalCategory {
    pub const ALL: [Self; 5] = [
        Self::Members,
        Self::Access,
        Self::Monitoring,
        Self::Alerts,
        Self::System,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Members => "members",
            Self::Access => "access",
            Self::Monitoring => "monitoring",
            Self::Alerts => "alerts",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.as_str() == value)
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Members => "journal.category_members",
            Self::Access => "journal.category_access",
            Self::Monitoring => "journal.category_monitoring",
            Self::Alerts => "journal.category_alerts",
            Self::System => "journal.category_system",
        }
    }
}

/// A line as the Journal page lists it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JournalEntry {
    pub id: i64,
    pub kind: JournalKind,
    pub actor_name: Option<String>,
    pub subject: Option<String>,
    pub detail: Option<String>,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl JournalEntry {
    /// What happened, in the reader's language.
    pub fn sentence(&self, i18n: &I18n) -> String {
        let key = match (self.kind, &self.subject) {
            (JournalKind::ChannelTested, None) => "journal.channel_tested_unnamed",
            (JournalKind::SignInFailed, None) => "journal.sign_in_failed_unknown",
            (kind, _) => kind.spec().key,
        };
        i18n.tf(
            key,
            &[
                ("actor", self.actor_name.as_deref().unwrap_or_default()),
                ("subject", self.subject.as_deref().unwrap_or_default()),
            ],
        )
    }
}

/// What the Journal page is narrowed to.
#[derive(Debug, Clone, Default)]
pub struct JournalFilters {
    pub category: Option<JournalCategory>,
    pub actor_id: Option<i64>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub q: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

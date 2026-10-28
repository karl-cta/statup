---
title: Services
description: "Les outils dont on dépend, qui les voit, leurs dossiers, leur état, comment il change de lui-même, et l'historique de leur disponibilité."
---

Un service est un outil dont on dépend au quotidien : la messagerie, le VPN, l'intranet, la téléphonie, un site web. Chacun affiche sur la page son état et les événements qui le citent.

Les éditeurs et les administrateurs gèrent les services depuis la page **Services**. Les lecteurs et les visiteurs les voient sur le tableau de bord, où un service s'ouvre dans un panneau latéral avec son état, ses 30 derniers jours et ses derniers événements.

## Créer un service

**Nouveau**, puis **Service**, sur la page Services, ouvre le formulaire :

| Champ | Description |
|---|---|
| **Nom** | Obligatoire, 100 caractères au plus. Deux services peuvent porter le même nom. |
| **Description** | Facultative, une phrase sur ce à quoi sert le service, 500 caractères au plus. |
| **Surveillance** | **Aucune**, **Web** ou **Port** : voir [Surveillance](monitoring.md). |
| **Icône** | Facultative : une icône fournie, ou une icône que vous envoyez. |
| **Visibilité** | Qui voit le service : voir [plus bas](#visibility). **Publique** par défaut. |

**Créer le service** l'ajoute en fin de liste, dans l'état **Opérationnel**. Les administrateurs peuvent aussi ajouter des services lors du [premier lancement](install.md#first-launch).

Pour modifier un service par la suite, ouvrez-le par son nom ou par son crayon sur la page Services, puis validez avec **Enregistrer**. Son état ne figure pas dans le formulaire : il se règle depuis la liste.

Pour créer d'un coup les services d'une page d'état d'Uptime Kuma, sous les noms de ses sondes : **Nouveau**, puis **Importer d'Uptime Kuma**. Voir [Uptime Kuma](monitoring-tools.md#uptime-kuma).

## Icônes

- 24 icônes fournies, de Serveur à Impression.
- **Envoyer une icône**, dans l'en-tête de la carte **Vos icônes** de la page de la bibliothèque d'icônes : PNG, JPG, WebP ou SVG, jusqu'à 256 Ko. Les images de plus de 128 pixels sont réduites ; un SVG est débarrassé de tout ce qui pourrait exécuter ou charger quelque chose.
- Les icônes envoyées forment une seule bibliothèque, partagée par les services, les événements et les modèles, accessible par **Gérer vos icônes** dans le choix d'icône d'un service. Une icône encore utilisée ne peut pas être supprimée.

## Visibilité

Chaque service dit qui le voit, dans son formulaire et dans le panneau d'un [dossier](#folders) :

| Valeur | Qui voit le service |
|---|---|
| **Publique** | Tous ceux qui peuvent voir la page, visiteurs sans compte compris. Par défaut. |
| **Membres** | Les membres connectés seulement. |
| **Restreinte** | Les groupes et les membres listés sous **Accès**, plus les éditeurs et les administrateurs. |

Quand la page est réservée aux membres, le choix est **Tous les membres** ou **Restreinte**.

Avec **Restreinte**, **Ajouter** ouvre une recherche parmi les [groupes](team.md#groups), avec leur nombre de membres, et parmi les lecteurs, par leur nom pour les éditeurs et avec leur adresse pour les administrateurs ; chacun s'affiche en étiquette, avec **Retirer**. Au-delà de cinq étiquettes, la liste se termine par « et 2 autres ». Une ligne dit qui verra le service, comme « Visible par 3 membres, plus les éditeurs et les administrateurs. » Un service restreint demande au moins un groupe ou un membre.

Les éditeurs et les administrateurs voient toujours tous les services. Pour les autres :

- Les incidents, maintenances et annonces suivent leurs services : un événement est vu par quiconque voit un des services qu'il cite, et ne montre que ceux-là. Une annonce qui ne cite aucun service est pour tout le monde.
- Un service qu'une personne ne peut pas voir ne laisse aucune trace pour elle : ni sur le tableau de bord ou le bandeau, ni dans les événements, la recherche ou les filtres, et un lien direct répond « introuvable ». Quand un visiteur ne voit aucun service, le bandeau indique « Aucun service visible ».
- Le [flux Atom](status-page.md#the-atom-feed) montre toujours ce que voit un visiteur sans compte.
- Le texte d'un événement (titre, description, mises à jour et bilan) s'affiche tel qu'il est écrit pour tous ceux qui voient l'événement : n'y nommez un service restreint que s'ils peuvent en avoir connaissance.

## Dossiers

Un dossier regroupe des services, comme Réseau ou Site de Lyon, sur un seul niveau : un dossier ne contient pas de dossier, et un service est dans un dossier au plus. Sur le tableau de bord, chaque dossier est un titre au-dessus de ses services, après les services hors dossier ; un lecteur ne voit le titre d'un dossier que s'il voit un de ses services.

Sur la page Services, chaque dossier est une carte avec son nom, son nombre de services et **Gérer le dossier** ; les services hors dossier viennent en premier, dans une carte sans titre.

### Créer un dossier

**Nouveau**, puis **Dossier**, en haut de la page Services, puis un **Nom du dossier** de 100 caractères au plus, unique, et **Créer le dossier**. La page du dossier s'ouvre.

### Page d'un dossier

- « Restreinte, Compta », à droite de son nom, avec **Modifier**, ouvre son **Nom du dossier** et sa visibilité, le même réglage que celui d'un service, puis **Enregistrer**.
- **Ajouter des services** liste les services qui ne sont pas dans ce dossier, avec une recherche au-delà de huit ; cochez-les, puis **Ajouter**. Un service rangé dans un autre dossier passe dans celui-ci.
- **Retirer**, sur la ligne d'un service, le sort du dossier.

### Visibilité héritée

Un service rangé dans un dossier hérite par défaut de sa visibilité : son formulaire montre **Hériter du dossier « Compta »**, coché, avec le réglage du dossier. Changer la visibilité du dossier change celle de tous les services qui en héritent. Décocher la case garde le réglage comme point de départ de celui du service.

Un dossier n'ouvre jamais un service par erreur :

- Un service ajouté à un dossier en hérite s'il était **Publique**, ou s'il avait déjà la visibilité du dossier. Un autre garde la sienne, et la page du dossier l'indique par **Visibilité propre**. Le message après l'ajout dit lesquels, comme « 3 services ajoutés. 2 héritent de la visibilité du dossier, 1 conserve la sienne. »
- Un service sorti d'un dossier, ou laissé par un dossier supprimé, garde la visibilité qu'il avait.

### Supprimer un dossier

**Supprimer le dossier**, au pied de sa page, avec **Supprimer** pour confirmer. Ses services restent, hors dossier, avec la visibilité qu'ils avaient.

## États

| État | À la main | Automatiquement |
|---|---|---|
| **Opérationnel** | Oui | |
| **Dégradé** | Oui | Par un incident ouvert d'impact **Mineur**, ou sans impact |
| **En panne** | Oui | Par un incident ouvert d'impact **Majeur**, ou par la [surveillance](monitoring.md) |
| **En maintenance** | Oui | Par une maintenance en cours, sauf si elle est marquée **Sans interruption** |

### Régler un état à la main

Sur la page Services, chaque service a un menu d'état. Un choix est publié aussitôt, avec **Revenir à l'état précédent** à côté. **En panne** demande d'abord « Publier l'état « En panne » ? », puis **Publier**.

### Quand un état change de lui-même

Un incident fixe l'état des services qu'il cite tant qu'il est **En analyse** ou **En correction**. Une maintenance le fait tant qu'elle est **En cours**. Une annonce, jamais. Voir [Incidents](incidents.md) et [Maintenances](maintenance.md).

Quand plusieurs causes s'appliquent, le service affiche la plus grave : **En panne**, puis **Dégradé**, puis **En maintenance**, puis **Opérationnel**. Une maintenance en cours ne masque pas une panne détectée, et un état réglé à la main sur **En panne** le reste pendant un incident mineur.

Quand l'incident passe **Sous surveillance**, est résolu ou annulé, ou quand la maintenance se termine, le service revient à la plus grave des causes restantes, en général l'état réglé à la main.

Tant qu'un événement ou une panne détectée fixe l'état d'un service, son menu laisse la place à l'état affiché, avec un lien vers cet événement, ou à **Détecté automatiquement** et **Déclarer un incident**. Le menu revient quand l'un ou l'autre prend fin.

## Ordre

**Organiser** place une flèche vers le haut et une vers le bas sur chaque service, qui le déplace dans sa carte, et sur chaque dossier, qui le déplace parmi les autres. L'ordre est enregistré peu après le dernier déplacement, et s'applique partout où les services sont listés, tableau de bord compris. **Terminer** ou Échap quitte ce mode.

Dans ce même mode, **Afficher d'abord les services perturbés** place chaque service qui n'est pas **Opérationnel** avant les autres, chaque groupe dans l'ordre choisi. Cette option est désactivée par défaut.

## Historique de disponibilité

Chaque service a une barre par jour, dans le fuseau horaire de l'instance, jusqu'à aujourd'hui. Le tableau de bord et la page Services montrent 90 jours quand la place le permet, et les 30 derniers sur un écran étroit ; le panneau latéral en montre toujours 30.

| Barre | Quand |
|---|---|
| Verte | Aucun incident ce jour-là |
| Orange | Un incident **Mineur** ou sans impact, ou une panne détectée de quinze minutes ou plus |
| Rouge | Un incident **Majeur** |
| Grise | Avant que le service existe |

- L'incident le plus grave de la journée l'emporte. Un incident compte de son début jusqu'à son passage **Sous surveillance**, ou jusqu'à sa fin ; un incident annulé ne compte jamais.
- Une maintenance ne colore jamais une journée.
- Sur ordinateur, une journée colorée par une panne détectée montre cette panne quand on clique dessus : voir [Surveillance](monitoring.md#in-the-history).
- Une panne détectée ne colore une journée que si aucun incident ne la couvre, et seulement si elle dure quinze minutes ou plus au total.

Aucun pourcentage : la légende compte les jours avec incident.

## Supprimer un service

Un service qu'aucun événement n'a jamais cité, et qu'aucune [maintenance répétée](maintenance.md#repeat) encore active ne cite, peut être supprimé depuis son formulaire, avec **Supprimer le service**. Il disparaît sans laisser de trace, pannes détectées comprises.

Un service cité par des événements est conservé, pour que leur historique reste complet. Pour ne plus le montrer sur le tableau de bord, un administrateur le décoche dans **Organiser** du tableau de bord : voir [Paramètres](settings.md#arrange-the-dashboard). Un service décoché revient dès qu'il n'est plus **Opérationnel**.

## Qui peut faire quoi

| | Lecteur | Éditeur | Administrateur |
|---|---|---|---|
| Voir les états et l'historique, ouvrir le panneau latéral d'un service | Oui | Oui | Oui |
| Créer, modifier, régler un état, changer l'ordre, supprimer, gérer les icônes | | Oui | Oui |
| Choisir qui voit un service, gérer les dossiers | | Oui | Oui |
| Lire pourquoi une panne a été détectée | Oui | Oui | Oui |
| Effacer une panne détectée qui n'en était pas une | | Oui | Oui |
| Retirer un service du tableau de bord | | | Oui |

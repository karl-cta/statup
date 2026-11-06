---
title: Incidents
description: "Déclarer ce qui est en panne, tenir tout le monde informé par des mises à jour, résoudre, puis écrire le bilan."
---

Un incident dit ce qui est en panne, quels services sont touchés et à quel point, et ce que l'équipe fait pour y remédier. Les éditeurs et les administrateurs en publient un depuis **Nouvel événement**, sur le tableau de bord ou sur la page Événements.

## Déclarer un incident

Dans le formulaire, avec **Type** sur **Incident** :

| Champ | Description |
|---|---|
| **Impact** | Obligatoire : **Mineur** ou **Majeur**. |
| **Services concernés** | Facultatif. Les services sont rangés sous leurs [dossiers](services.md#folders), chaque dossier avec une case qui coche ses services ; au-delà de huit services, une recherche filtre la liste, et la case ne coche alors que les services affichés. Une ligne sous la liste dit ce qui va changer, comme « Messagerie : en panne. » |
| **Titre** | Obligatoire, 200 caractères au plus. Tant que vous n'en tapez pas, il s'écrit tout seul, comme « Panne sur Messagerie ». |
| **Description** | Facultative : ce qui est touché, ce que l'on sait. |

Sous **Plus d'options** :

- **Avancement** : **En analyse** par défaut, ou **En correction** ou **Sous surveillance**.
- **Début réel**, si l'incident a commencé plus tôt. Il ne peut pas être dans le futur.
- **Prochaine mise à jour** : voir [plus bas](#next-update).
- **Enregistrer comme modèle** : voir [Modèles](#templates).

**Publier l'incident** l'affiche aussitôt et l'envoie aux [canaux d'alerte](alerts.md) qui reçoivent les incidents.

La description, les mises à jour et le bilan acceptent une mise en forme simple : `**bold**`, `*italic*`, listes, `[links](https://example.com)`, code et tableaux. Pas d'images.

### Depuis une panne détectée

Quand la [surveillance](monitoring.md) trouve un service en panne, **Déclarer un incident**, sur le bandeau ou sur la page Services, ouvre le formulaire avec l'impact **Majeur**, ce service, et le début de la panne comme début réel.

## L'effet sur les services

Tant que l'incident est **En analyse** ou **En correction**, les services qu'il cite affichent **En panne** pour un impact **Majeur**, **Dégradé** pour un impact **Mineur**. À partir de **Sous surveillance**, ils reviennent à leur propre état. Voir [Services](services.md#states).

## Avancement

| Avancement | Signifie | Peut passer à |
|---|---|---|
| **En analyse** | L'équipe examine le problème. | En correction, Sous surveillance, Résolu, Annulé |
| **En correction** | La correction est en cours. | Sous surveillance, Résolu, Annulé |
| **Sous surveillance** | Le service est rétabli et surveillé. La chronologie indique « Service rétabli ». | En correction, Résolu |
| **Résolu** | Terminé. L'incident indique combien de temps il a duré. | |
| **Annulé** | Clos sans compter dans l'historique des services. | |

**Revenir à l'avancement « En analyse »**, ou à l'état qui précédait, annule le dernier changement, d'un seul cran, sur la page de l'incident.

## Publier une mise à jour

**Publier une mise à jour**, sur la page de l'incident ou dans son panneau latéral, prend un message, un nouvel avancement, ou les deux, ainsi que la prochaine mise à jour. **Publier** l'ajoute à la chronologie et l'envoie aux canaux d'alerte.

Chaque mise à jour affiche son heure. Le nom de son auteur n'est montré qu'aux membres connectés.

## Prochaine mise à jour

Annoncer l'heure de la prochaine mise à jour indique aux lecteurs quand revenir voir.

- Un délai : `30`, `30 min`, `1 h`, `1h30`. Écrit en heures, un délai va jusqu'à `4h59` : à partir de `5h`, il se lit comme une heure. Écrit en minutes, jusqu'à `999`.
- Une heure : `14:30`, `14h30`, `9h`. Elle désigne le prochain passage à cette heure, aujourd'hui ou demain, dans le fuseau horaire de l'instance.

Le bandeau indique alors « mise à jour prévue à 14:30 », puis « mise à jour attendue depuis 5 min » une fois en retard. La page de l'incident indique « dernière mise à jour il y a 9 min, prochaine prévue à 14:30 ».

Chaque mise à jour fixe à nouveau la suivante : laissé vide, le champ l'efface. Un délai est proposé de nouveau à la mise à jour suivante ; une heure ne l'est pas. Résoudre ou annuler l'incident la retire.

### Rappel

Quand une mise à jour a plus de 5 minutes de retard, le membre qui l'a annoncée reçoit une notification, « Mise à jour en retard », et un e-mail si un serveur SMTP est configuré. Une seule fois pour chaque heure annoncée.

## Résoudre

Choisissez **Résolu** dans **Publier une mise à jour**, avec ou sans message. Les services reviennent à leur propre état, et l'incident affiche « a duré 2 h ». Un message publié à moins de deux minutes de la résolution s'affiche avec elle, comme un seul moment de la chronologie.

## Écrire le bilan

Une fois l'incident **Résolu**, **Écrire le bilan** permet de raconter ce qui s'est passé, pourquoi, et ce qui change pour que cela ne se reproduise pas. **Publier le bilan** l'affiche en tête de la page de l'incident et dans son panneau latéral, marque l'incident « Résolu · bilan » dans les listes, l'ajoute au flux et l'envoie une seule fois aux canaux d'alerte.

**Modifier**, dans la carte **Bilan**, le corrige sans rien renvoyer ; **Supprimer** l'efface. Un incident annulé n'a pas de bilan. Si un administrateur rouvre l'incident, son bilan est masqué jusqu'à ce qu'il soit de nouveau résolu.

## Modifier ou supprimer

**Modifier** change le titre, la description, l'impact et les services. Le type, le début réel et la prochaine mise à jour restent tels quels. Une modification n'envoie aucune alerte.

**Supprimer l'événement**, en bas de la page de modification, le retire partout, mises à jour et bilan compris, sans envoyer d'alerte. Une mise à jour peut être supprimée seule : par son auteur tant que l'incident est ouvert, ou par un administrateur.

Une fois un incident **Résolu** ou **Annulé**, seul un administrateur peut encore le modifier, y publier, changer son avancement ou le supprimer. Les éditeurs peuvent toujours écrire son bilan.

## Modèles

Un modèle garde le titre, la description, le type, l'impact et la catégorie d'un événement, sans ses services ni ses horaires.

- Cochez **Enregistrer comme modèle** en publiant un nouvel événement.
- **Partir d'un modèle**, en haut du formulaire, liste les 30 plus utilisés. Taper un titre de deux lettres ou plus propose aussi des modèles enregistrés.
- En appliquer un remplit le titre, la description, le type, l'impact et la catégorie. Les services et les horaires restent à choisir.
- Un modèle se supprime depuis la liste **Partir d'un modèle**.

## Qui peut faire quoi

| | Lecteur | Éditeur | Administrateur |
|---|---|---|---|
| Lire les incidents | Oui | Oui | Oui |
| Déclarer, mettre à jour, modifier ou supprimer un incident ouvert | | Oui | Oui |
| Écrire, corriger ou retirer le bilan | | Oui | Oui |
| Modifier un incident résolu ou annulé | | | Oui |

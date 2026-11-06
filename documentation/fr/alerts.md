---
title: Alertes
description: "Publier chaque incident, maintenance et annonce dans Teams, Slack, Google Chat, Discord, Mattermost, ntfy, par e-mail ou dans tout autre outil."
---

Statup publie chaque incident, maintenance et annonce là où les gens lisent déjà. Les administrateurs règlent les canaux dans **Paramètres**, **Alertes**.

Dans l'application, **Notifications** désigne la cloche de l'en-tête ; les **Alertes** sont les messages envoyés vers d'autres outils.

## Ajouter un canal

**Nouveau canal**, en tête de la carte **Canaux**, ouvre le formulaire :

| Champ | Description |
|---|---|
| **Type** | Microsoft Teams, Slack, Google Chat, Discord, Mattermost, ntfy (téléphone), E-mail ou Webhook. |
| **Nom dans Statup** | Obligatoire, 60 caractères au plus : le nom du canal dans les listes de Statup, comme « Mon téléphone ». Son exemple suit le type. |
| **URL du webhook** | Ou **URL du sujet** pour ntfy, **Adresses** pour l'e-mail. Voir plus bas où la trouver. |
| **Langue des messages** | Français, English, Deutsch ou Español. Au départ, la [langue par défaut](configuration.md#default_locale) de l'instance. |
| **Reçoit** | **Incidents**, **Maintenances** et **Annonces** sont cochés au départ, **Pannes détectées automatiquement** ne l'est pas. |

**Envoyer un test** publie un message de test avec les valeurs du formulaire, sans les enregistrer, et affiche « Message de test envoyé. » ou la raison de l'échec. Un canal sans aucune case cochée sous **Reçoit** est **En pause**.

Pour modifier un canal, ouvrez-le par son nom dans la liste. **Supprimer ce canal** arrête tout message vers lui, y compris ceux en attente.

## Où trouver l'URL

| Type | À coller |
|---|---|
| Microsoft Teams | Dans le canal : ⋯, Workflows, puis le modèle qui publie une requête webhook dans le canal. Les anciens webhooks entrants (connecteurs Office 365) ne fonctionnent plus depuis mai 2026. |
| Slack | Sur api.slack.com/apps, créez une app, activez Incoming Webhooks et ajoutez-la au canal. Rocket.Chat accepte le même format : choisissez Slack. |
| Google Chat | Dans l'espace Google Chat : Applications et intégrations, Webhooks, Ajouter. |
| Discord | Paramètres du salon : Intégrations, Webhooks, Nouveau webhook, Copier l'URL du webhook. |
| Mattermost | Dans Mattermost : Intégrations, Webhooks entrants, Ajouter. |
| ntfy | La case commence par `https://ntfy.sh/` : tapez le sujet à la suite, ou le sujet seul. Sur votre propre serveur ntfy, l'URL complète du sujet. Puis abonnez-vous au sujet dans l'application ntfy. Un sujet sur ntfy.sh est lisible par quiconque connaît son nom : choisissez-en un que personne ne devinera, ou utilisez votre propre serveur ntfy, avec `?auth=` dans l'URL pour un sujet protégé. |
| E-mail | Des adresses séparées par des virgules, 50 au plus. |
| Webhook | Une URL qui accepte un `POST` JSON : Zapier, Make, n8n, Power Automate, Home Assistant. Voir [Corps du webhook](webhook.md). |

## E-mail

Les canaux E-mail ont besoin d'un serveur SMTP : définissez [`SMTP_HOST`](configuration.md#smtp_host) et les autres réglages dans `.env`. Sans lui, le type s'affiche « E-mail (serveur SMTP non configuré) ».

- Chaque adresse reçoit son propre exemplaire : une adresse refusée par le serveur n'empêche personne d'autre de recevoir le message.
- Tous les messages d'un même événement ont le même objet et se regroupent en une seule conversation dans la messagerie.
- Le même serveur envoie les liens de [mot de passe oublié](forgotten-password.md), et prévient une fois un membre quand une mise à jour qu'il a annoncée sur un incident a plus de 5 minutes de retard.

## Envoi

- Les messages partent en quelques secondes, dans l'ordre pour chaque canal.
- Un canal qui ne répond pas est réessayé après 30 secondes, puis 2, 10 et 30 minutes. Au-delà, ou tout de suite quand le service refuse l'URL (404, 401), ses messages en attente sont abandonnés.
- La liste des canaux indique, pour chacun, son dernier envoi, sa prochaine tentative et sa raison, ou son échec et sa raison.

Les pannes trouvées par la [surveillance](monitoring.md), ou signalées par un [outil de supervision](monitoring-tools.md), dans la même minute sont regroupées en un message par canal, et leur retour en un message de plus.

## Abonnés par e-mail

Les visiteurs et les membres s'abonnent eux-mêmes depuis la page **S'abonner**, décrite dans [la page d'état](status-page.md#get-notified). La carte **Abonnés par e-mail** de la page Alertes les compte et mène à leur liste.

- **Voir les abonnés** liste chaque personne : qui elle est, ce qu'elle reçoit, depuis quand, et comment s'est passé son dernier e-mail, avec la raison s'il a échoué. **Retirer** met fin à un abonnement après une question sous le bouton ; la personne ne reçoit plus rien.
- **Les visiteurs peuvent s'abonner** est coché par un administrateur et enregistré avec **Enregistrer**. C'est décoché sur une instance neuve. Le décocher arrête les e-mails aux visiteurs déjà abonnés, qui reviennent quand la case est recochée ; les membres continuent de recevoir les leurs.
- Rien n'est envoyé sans serveur SMTP ([`SMTP_HOST`](configuration.md#smtp_host)) : la carte le dit alors et ne propose pas d'interrupteur.
- Le [Journal](journal.md) garde l'adresse d'un abonné qui a confirmé, est parti ou a été retiré, aussi longtemps qu'il garde ses lignes.

## URL de la page

Chaque message renvoie vers la page. En bas de la page Alertes, **URL de la page** indique cette adresse :

- Si [`PUBLIC_URL`](configuration.md#public_url) est défini, c'est cette adresse, et elle ne peut pas être changée ici.
- Sinon, elle peut être saisie ici. Le premier canal ajouté la remplit avec l'adresse à laquelle la page Alertes a été ouverte ; une adresse vidée volontairement reste vide.

## Garder les adresses secrètes

Quiconque connaît l'adresse d'un webhook peut publier dans son canal. Statup ne montre les adresses qu'aux administrateurs et ne les écrit jamais dans les logs ni dans le Journal. Vos [sauvegardes](backups.md) les contiennent.

---
title: Paramètres
description: "Le nom, le logo, les couleurs, le fuseau horaire et le public de la page, la version, et l'organisation du tableau de bord."
---

La page **Paramètres**, réservée aux administrateurs, liste six pages sous **Gestion**, à côté de la carte **Général** sur un grand écran, au-dessus sinon :

- **Équipe** : les membres et leurs rôles. Voir [Équipe et rôles](team.md).
- **Alertes** : les canaux qui reçoivent les nouvelles. Voir [Alertes](alerts.md).
- **API** : les clés des scripts et des outils qui utilisent l'API. Voir [API](api.md#keys).
- **Journal** : ce qui s'est passé sur l'instance. Voir [Journal](journal.md).
- **Sauvegardes** : l'horaire des sauvegardes automatiques, et les sauvegardes du serveur. Voir [Sauvegardes](backups.md).
- **Version** : la version en service.

## Général

Un seul formulaire, enregistré par un seul bouton **Enregistrer**. Tous les champs sont vérifiés avant qu'aucun ne soit écrit, et si vous quittez la page avec des changements non enregistrés, le navigateur demande d'abord confirmation.

| Champ | Description |
|---|---|
| **Nom de la page** | Affiché à la place de Statup dans l'en-tête et l'onglet du navigateur. 40 caractères au plus ; vide, il affiche Statup. |
| **Logo** | Remplace le signe Statup dans l'en-tête ; « Propulsé par Statup » reste en bas de page. PNG, JPG, WebP ou SVG, jusqu'à 256 Ko. Une image de plus de 256 pixels est réduite. **Retirer le logo** remet aussitôt le signe. |
| **Couleur** | Colore les boutons principaux, les cases cochées, les choix sélectionnés, le trait sous l'en-tête et sous l'onglet courant, et le point et la dernière lettre du signe Statup quand aucun logo n'est choisi, sur toutes les pages et pour tous les lecteurs. Douze couleurs : **Encre** (par défaut), **Marine**, **Cobalt**, **Pétrole**, **Sapin**, **Rose**, **Violet**, **Bordeaux**, **Brique**, **Cuivre**, **Cacao** et **Ardoise**, chacune dessinée pour le thème clair et le thème sombre. Les couleurs des états ne changent jamais. |
| **Ambiance** | Le fond et les cartes de toutes les pages : **Sable** (par défaut, une page blanche aux cartes crème), **Brume** (teintée de bleu), **Papier** (crème) ou **Galet** (une page grise aux cartes blanches), chacune avec sa version sombre. Chaque lecteur garde le bouton clair ou sombre de l'en-tête. |
| **Fuseau horaire** | Les heures de la page, des événements et des maintenances s'affichent et se saisissent dans ce fuseau. |
| **Accès à la page** | **Membres seulement** ou **Publique**. Voir [Équipe et rôles](team.md#page-access). |

Une couleur ou une ambiance s'affiche sur la page des paramètres dès qu'elle est choisie, et sur toutes les pages une fois enregistrée.

Ce qui est enregistré ici prime sur [`TZ`](configuration.md#tz) et [`PUBLIC_MODE`](configuration.md#public_mode), et est conservé d'un redémarrage à l'autre.

## Version

La ligne **Version** indique la version en service et mène aux versions publiées sur GitHub. Quand une version plus récente est publiée, elle affiche « La version 0.7.0 est disponible. » et mène à ses nouveautés.

Statup interroge GitHub une fois par jour, la première fois une minute après son démarrage. La réponse est gardée en mémoire : après un redémarrage, rien ne s'affiche avant cette première interrogation. [`UPDATE_CHECK=false`](configuration.md#update_check) la désactive.

## Organiser le tableau de bord

**Organiser**, à côté de **Nouvel événement** sur le tableau de bord, permet à un administrateur de disposer la page que tout le monde lit. Ce mode est proposé sur les écrans de 768 pixels de large ou plus : un téléphone empile tous les blocs sur une seule colonne.

| Bloc | Description |
|---|---|
| **Bandeau d'état** | Toujours affiché, sur toute la largeur, au-dessus des autres. |
| **Services** | Un quart de la largeur par défaut. Ses options choisissent les services listés. Un service masqué réapparaît dès qu'il est perturbé. |
| **Activité récente** | La moitié de la largeur par défaut. Ses options choisissent ce qui est listé : **Incidents mineurs**, **Incidents majeurs**, **Maintenances**, **Annonces**. Tout sauf les maintenances par défaut. |
| **Maintenances** | Un quart de la largeur par défaut. |

- Déplacez un bloc en le faisant glisser, ou avec les flèches du clavier.
- Réglez sa largeur : **Un quart**, **La moitié** ou **Toute la largeur**.
- **Options**, sur un bloc, ouvre le choix de ce qu'il affiche. **Masquer** le retire de la page ; **Ajouter un bloc** le fait revenir.

Chaque changement est enregistré aussitôt, pour tous les lecteurs : il n'y a qu'une organisation par instance. **Terminer**, ou Échap, quitte ce mode.

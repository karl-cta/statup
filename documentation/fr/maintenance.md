---
title: Maintenances
description: "Annoncer des travaux à l'avance, laisser la maintenance commencer et finir d'elle-même, ou la lancer tout de suite."
---

Une maintenance prévient que des travaux sont prévus ou en cours, sur quels services, et jusqu'à quand. Les éditeurs et les administrateurs en publient une depuis **Nouvel événement**, avec **Type** sur **Maintenance**.

## Annoncer une maintenance

| Champ | Description |
|---|---|
| **Début** | **Maintenant** par défaut. Choisissez un jour et une **Heure de début** pour l'annoncer à l'avance : la liste propose chaque demi-heure, et toute heure peut être saisie. |
| **Durée** | En heures et minutes. La **Fin estimée** suit, comme « Demain à 02:00 ». Sans durée : « Sans fin prévue ». |
| **Répéter** | Proposé dès qu'un jour est choisi, jamais avec **Maintenant**, et seulement à la création de la maintenance. Voir [Répéter](#repeat). |
| **Services concernés** | Les services touchés par les travaux. |
| **Titre** | Obligatoire. Tant que vous n'en tapez pas, il s'écrit tout seul, comme « Maintenance de Messagerie ». |
| **Description** | Facultative. |

Les heures sont dans le fuseau horaire de l'instance, que le formulaire indique. Sous **Plus d'options**, **Sans interruption** garde les services utilisables pendant les travaux : leur état ne change jamais. Ce choix ne se fait qu'à la création de la maintenance.

**Annoncer la maintenance** la publie et l'envoie aux [canaux d'alerte](alerts.md) qui reçoivent les maintenances.

## À l'avance, ou tout de suite

- **Avec une heure de début**, la maintenance est annoncée : **Programmée**. Ses services ne changent pas avant qu'elle commence.
- **Sans heure de début**, elle commence aussitôt : **En cours**.

## Début et fin automatiques

Chaque minute, Statup démarre chaque maintenance programmée dont l'heure de début est arrivée, et termine chaque maintenance en cours dont l'heure de fin est arrivée. Le début comme la fin sont enregistrés aux heures annoncées, et chacun envoie son alerte.

Une maintenance sans durée ne se termine jamais d'elle-même : terminez-la à la main. Un début ou une fin automatique ne peut pas être annulé.

## À la main

Dans **Publier une mise à jour**, choisissez :

- **En cours** pour la commencer maintenant ;
- **Terminée** pour la terminer maintenant, plus tôt que prévu ;
- **Annulée** pour l'annuler, avant ou pendant les travaux.

## La déplacer

Tant qu'elle est programmée, **Modifier** change son début ou sa durée, et les canaux d'alerte reçoivent le nouveau créneau (« Nouvelle date : … »). Une fois commencée, son début est verrouillé ; sa durée peut encore changer.

## Répéter

**Répéter** ramène les mêmes travaux chaque semaine ou chaque mois. Ses deux choix portent le nom du jour choisi : « Chaque dimanche », ou « Le premier dimanche du mois » pour le rang de ce jour dans son mois. Le rang vient du jour du mois : du 1er au 7, le premier ; du 8 au 14, le deuxième ; du 15 au 21, le troisième ; du 22 au 28, le quatrième ; du 29 au 31, le dernier. Pour « le dernier », choisissez une date du 29 au 31.

Une durée est obligatoire : sans elle, le formulaire répond « Une maintenance qui se répète a besoin d'une durée. »

Une maintenance répétée est annoncée une seule fois. Les [canaux d'alerte](alerts.md) et les abonnés par courriel reçoivent un message, à la création, avec le rythme après le créneau, comme « Chaque dimanche ». Ensuite, chaque séance commence et se termine d'elle-même, en silence : aucun message à son début ni à sa fin. La [surveillance](monitoring.md#when-no-outage-is-declared) reste muette pendant chaque séance, comme pour toute maintenance. **Sans interruption** et les services valent pour toutes les séances.

Statup garde une séance programmée d'avance. Dès qu'elle commence, ou qu'elle est annulée ou supprimée, la suivante apparaît ; sa chronologie s'ouvre sur **Ajoutée automatiquement**, sans auteur. Les séances prévues pendant que le serveur était arrêté ne sont pas rattrapées : la suivante est la première encore à venir.

Les heures suivent l'horloge de l'instance : 02:00 reste 02:00 d'un changement d'heure à l'autre. La nuit où une heure n'existe pas, la séance commence à la première minute qui suit.

Sur la page d'une séance, la carte **Détails** montre **Répétition** avec le rythme. Les éditeurs et les administrateurs y trouvent **Arrêter la répétition** : elle demande une confirmation, annule la séance programmée d'avance (son annulation est envoyée comme n'importe quelle autre) et n'en ajoute plus. Une séance en cours va jusqu'à sa fin.

**Modifier** ne change qu'une séance. Cocher **Appliquer aussi aux suivantes** donne son titre, sa description, ses services, son heure et sa durée aux séances qui la suivent, y compris celle déjà programmée. Le jour où elle tombe devient le jour du rythme, ou son rang dans le mois.

Annuler une séance depuis **Publier une mise à jour** saute cette date seulement : la séance suivante apparaît. Déplacer une séance à la main envoie « Nouvelle date : … » comme d'habitude.

L'arrêt, et **Appliquer aussi aux suivantes**, sont inscrits au [Journal](journal.md).

Il n'y a que ces deux rythmes, et pas de date de fin : arrêtez-la à la main. Une répétition ne se règle pas par l'[API](api.md), qui lit chaque séance comme une maintenance ordinaire.

## Ce qu'affichent les services

Tant qu'elle est en cours, les services qu'elle cite affichent **En maintenance**, sauf si elle est marquée **Sans interruption**, ou si un état plus grave l'emporte. Une maintenance programmée ne change encore rien.

La [surveillance](monitoring.md#when-no-outage-is-declared) ne déclare aucune panne pour un service en maintenance.

## Sur la page

- Quand rien n'est perturbé, le bandeau indique « Maintenance en cours » pendant les travaux, et « Maintenance prévue le … » quand une maintenance unique commence dans les sept jours. Une maintenance répétée n'y est jamais annoncée d'avance : le bandeau montre une séance seulement pendant qu'elle est en cours.
- Le bloc **Maintenances** liste celles en cours, puis les programmées par date de début, puis les cinq dernières terminées ou annulées. Une maintenance répétée y figure par sa séance programmée et sa dernière séance terminée seulement, pas par chaque séance passée.
- La page de la maintenance indique « dans 2 j » avant qu'elle commence et « commencée il y a 20 min » pendant les travaux, avec le début et la fin prévus sur sa chronologie.

## Après les travaux

Une fois une maintenance terminée, **Publier les nouveautés** ouvre une annonce déjà remplie : « Nouveautés après … », dans la catégorie **Nouveauté**, sur les mêmes services, et faisant suite à cette maintenance. Voir [Annonces](announcements.md).

## Modifier ou supprimer

Les règles sont celles des [incidents](incidents.md#edit-or-delete) : une fois une maintenance **Terminée** ou **Annulée**, seul un administrateur peut encore la modifier ou la supprimer.

---
title: Journal
description: "Ce qui s'est passé sur l'instance, jour par jour, conservé pour les administrateurs et exportable en fichier CSV."
---

Le Journal liste ce qui s'est passé sur l'instance, jour par jour. Les administrateurs l'ouvrent depuis **Paramètres**, **Journal**, puis **Ouvrir le journal**. La sortie du serveur (`docker compose logs statup`) montre les mêmes lignes, en anglais.

## Niveau de capture

**Modifier**, en haut, règle le niveau de capture et la rétention.

**Essentiel**, par défaut, garde les modifications importantes et les erreurs :

- les services ajoutés, modifiés ou supprimés, les paramètres, les canaux d'alerte, les abonnés par e-mail retirés et les abonnements des visiteurs ouverts ou fermés ;
- les membres ajoutés, un changement de rôle ou de compte, un événement supprimé ;
- les groupes et les dossiers créés, renommés ou supprimés, les membres d'un groupe, les services d'un dossier, et qui voit un service ou un dossier ;
- une panne détectée, terminée ou effacée, un signalement d'outil de supervision ignoré, les vérifications en pause quand Statup n'a plus de réseau ou un passage ignoré, et des messages vers un canal d'alerte ou des e-mails aux abonnés abandonnés ou en attente ;
- les clés d'API, le lien du widget, l'horaire des sauvegardes, une sauvegarde téléchargée ou restaurée, une sauvegarde automatique en échec, et une maintenance répétée arrêtée ou modifiée avec les suivantes ;
- les connexions, les vérifications du mot de passe actuel et les demandes de réinitialisation bloquées après trop de tentatives, un e-mail de réinitialisation non envoyé, et les erreurs de l'application.

**Détaillé** garde toutes les actions, à partir de l'activation : connexions et tentatives refusées, réinitialisations de mot de passe, événements publiés et mis à jour, chaque vérification échouée, chaque message envoyé.

La **Rétention** est de 90 jours par défaut, de 1 à 365. Personne ne peut modifier ni supprimer une entrée : seule la rétention les efface.

## Trouver une entrée

**Rechercher** parcourt les entrées. **Type** les restreint à **Membres**, **Connexions**, **Surveillance**, **Alertes** ou **Système**, **Membre** à une personne, et **Du** et **Au** à une période.

## Exporter

**Exporter** télécharge les entrées affichées, filtres compris, dans un fichier CSV en UTF-8 nommé `journal-YYYY-MM-DD.csv`, de 10 000 entrées au plus. Un tableur l'ouvre directement.

## Ce qui n'est jamais écrit

Les entrées de connexion gardent l'adresse du client, lue comme dans le reste de l'application : voir [`TRUST_PROXY_HEADERS`](configuration.md#trust_proxy_headers). Aucun mot de passe, lien de réinitialisation, adresse de webhook, ni rien de ce qui suit l'hôte d'une adresse vérifiée n'est jamais écrit.

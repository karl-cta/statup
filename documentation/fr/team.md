---
title: Équipe et rôles
description: "Lecteurs, éditeurs et administrateurs, comment leurs comptes sont créés, les groupes auxquels ils appartiennent, et qui peut voir la page."
---

## Rôles

| Rôle | Peut |
|---|---|
| **Lecteur** | Lire le tableau de bord, les événements et le flux, et modifier son propre profil. |
| **Éditeur** | Tout ce que peut un lecteur, plus publier incidents, maintenances et annonces, régler l'état des services, et gérer les services, les dossiers, les modèles et les icônes. |
| **Administrateur** | Tout, plus les paramètres, l'équipe et ses groupes, les alertes, le Journal, l'organisation du tableau de bord, et la modification ou la suppression des événements clos. |

## Accès à la page

Se choisit au [premier lancement](install.md#first-launch), puis dans **Paramètres**, sous **Accès à la page** :

- **Membres seulement** : il faut se connecter, et les lecteurs de flux ne peuvent plus lire le flux.
- **Publique** : les visiteurs lisent la page et le flux sans compte.

## Ajouter un membre

Les comptes sont créés par un administrateur : personne d'autre ne peut s'inscrire, sauf sur une instance vide, pour en devenir le premier administrateur.

**Paramètres**, **Équipe**, puis **Nouveau membre** en tête de la carte **Membres** : **Nom**, **Adresse e-mail** et **Rôle**, **Lecteur** par défaut. **Créer le compte** affiche une seule fois un mot de passe provisoire, avec **Copier**.

- Aucun e-mail n'est envoyé : transmettez vous-même le mot de passe. Il n'est affiché que cette fois-là et n'est conservé nulle part.
- Il reste valable sept jours. À sa première connexion, le membre doit choisir le sien, sur **Choisissez votre mot de passe**.
- Une adresse déjà utilisée par un autre compte, même désactivé, est refusée.

## Règle des mots de passe

Tout mot de passe choisi doit faire 12 caractères avec majuscule, minuscule, chiffre et caractère spécial, ou au moins 20 caractères quels qu'ils soient. Les mots de passe sont hachés avec Argon2id.

## Changer un rôle

**Gérer** sur la ligne d'un membre, puis le rôle sous **Rôle** et **Enregistrer**. Le rôle de chacun est écrit sous son nom. Un membre désactivé garde son rôle, qui se change une fois le compte réactivé. Personne ne peut changer son propre rôle, et le dernier administrateur ne peut pas en changer : il reste toujours au moins un administrateur actif.

## Réinitialiser un mot de passe

**Gérer** sur sa ligne, puis **Réinitialiser** sous **Réinitialiser le mot de passe**, donne à un membre actif un nouveau mot de passe provisoire, valable sept jours. L'ancien ne fonctionne plus et ses sessions ouvertes sont fermées. Pour votre propre mot de passe, passez par **Mon profil**. Quand plus personne ne peut se connecter, la personne qui héberge Statup peut le faire en [ligne de commande](command-line.md).

## Désactiver un membre

**Gérer** sur sa ligne, puis **Désactiver**, empêche un membre de se connecter ; ses sessions ouvertes prennent fin à son prochain clic. **Réactiver**, au même endroit, lui rend l'accès. Son adresse reste réservée entre-temps. Personne ne peut se désactiver soi-même, ni désactiver le dernier administrateur actif.

Les membres ne peuvent pas être supprimés : désactivez-les plutôt.

## Groupes

Un groupe réunit des lecteurs qui voient les mêmes services restreints, comme Compta ou Site de Lyon. Un groupe ne fait qu'ouvrir des services : voir [Visibilité](services.md#visibility). Les éditeurs et les administrateurs voient tous les services, un groupe ne contient donc que des lecteurs.

Les administrateurs gèrent les groupes depuis **Paramètres**, **Équipe**, dans la carte **Groupes**, qui liste chaque groupe avec son nombre de membres.

### Créer un groupe

**Nouveau groupe**, puis un **Nom du groupe** de 100 caractères au plus, unique, et **Créer le groupe**. La page du groupe s'ouvre. **Renommer**, à droite de son nom, le change.

### Membres d'un groupe

Sur la page du groupe, **Ajouter des membres** liste les lecteurs qui n'en font pas encore partie, avec une recherche au-delà de huit ; cochez-les, puis **Ajouter**. **Retirer**, sur la ligne d'un membre, le sort du groupe aussitôt. Un lecteur peut appartenir à plusieurs groupes.

### Services visibles

La carte **Services visibles** liste ce que le groupe ouvre : les [dossiers](services.md#folders) restreints, avec leur nombre de services, puis les services restreints qui ont leur propre visibilité.

**Ajouter des services** ne propose que des dossiers et des services restreints : ajouter les ouvre au groupe et ne les cache à personne. Un service qui hérite de son dossier s'ouvre par son dossier. **Retirer** supprime l'accès ; un service que plus aucun groupe ni membre ne peut voir n'est alors visible que par les éditeurs et les administrateurs, et la page dit combien.

### Supprimer un groupe

Sous **Supprimer le groupe**, au pied de sa page, **Supprimer** demande une confirmation. Quand des services ne sont visibles que par ce groupe, la carte dit combien : ils ne sont alors plus visibles que par les éditeurs et les administrateurs, jusqu'à ce que leur visibilité dise autre chose. Les membres gardent leur compte.

## Mon profil

Chaque membre accède à **Mon profil** depuis son menu :

- **Informations du compte** : son nom et son adresse e-mail. Une nouvelle adresse demande le mot de passe actuel, puisque les liens de réinitialisation y sont envoyés.
- **Mot de passe** : l'actuel, puis le nouveau deux fois. Les autres sessions du compte sont fermées.

La langue se choisit avec le sélecteur de langue, et reste enregistrée sur le compte. Le thème clair ou sombre est gardé par chaque navigateur.

## Sessions

Cocher **Rester connecté 30 jours** à la connexion garde la session 30 jours sans visite ; sinon, elle prend fin au bout de 24 heures.

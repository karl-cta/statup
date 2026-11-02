---
title: Widget d'état
description: "Un petit bloc à coller dans un intranet ou un wiki, qui reprend le bandeau de la page d'état et se rafraîchit seul."
---

Le widget est un bloc de 68 px de haut qui prend la largeur qu'on lui donne, sur deux lignes. Le bloc entier est un lien qui ouvre la page d'état dans un nouvel onglet (**Voir**). Les administrateurs en prennent le code dans **Paramètres**, **Widget**, entre **Alertes** et **API**.

## Ce qu'il montre

Le widget reprend le bandeau de la [page d'état](status-page.md), avec les mêmes règles :

- Tout fonctionne : **Tous les services fonctionnent normalement**, et dessous le nom de la page et **État des services**.
- Une perturbation : le titre de l'incident ouvert le plus grave, **+ 1 autre** quand d'autres problèmes sont ouverts, et dessous le service et son état, ou **2 services perturbés** quand il y en a plusieurs. La ligne dit aussi quand l'incident a été mis à jour pour la dernière fois. Le widget prend la teinte de l'état, comme le bandeau.
- Une maintenance en cours : son titre, le service et jusqu'à quand elle dure, sur une teinte bleue.
- Une maintenance annoncée dans les sept jours qui viennent, quand rien n'est perturbé : **Maintenance prévue le** et sa date. Une maintenance répétée n'est pas annoncée ici.
- Un service que les vérifications ont trouvé en panne sans incident : le service, son état et depuis quand.

Le widget se rafraîchit seul chaque minute, sans aucun script. Dans une colonne étroite, le texte est coupé par des points de suspension ; **+ 1 autre** et **Voir** restent.

Le widget a sa propre limite de requêtes, à part de celle des pages : 6 000 par minute depuis une même adresse, de quoi le laisser ouvert dans tout un bureau derrière une seule adresse d'entreprise.

## Obtenir le code

**Paramètres**, **Widget**, comporte trois parties :

- **Aperçu** montre le widget avec l'état du moment, tel qu'un membre le voit, en **Clair**, **Sombre** ou **Selon l'appareil**. Le thème choisi change l'adresse et le code en dessous.
- **À coller** donne l'**Adresse**, avec **Copier l'adresse**, et la hauteur conseillée, 68 px. **Code HTML** contient une `iframe` qui pointe vers l'adresse, avec **Copier le code**.
- **Changer le lien**, en rouge au pied de la page, est décrit dans [Le lien privé](#the-private-link).

L'adresse commence par [`PUBLIC_URL`](configuration.md#public_url) quand il est défini, sinon par l'**URL de la page** réglée dans [Alertes](alerts.md#page-url), sinon par l'adresse qu'a utilisée le navigateur de l'administrateur. Définissez l'une des deux premières avant de copier, pour que le widget pointe vers l'adresse que vos collègues atteignent.

## Le coller

Une page qui accepte du HTML prend le **Code HTML** :

```html
<iframe src="https://status.example.com/embed?key=YOUR-PRIVATE-KEY&amp;lang=en" title="Service status" width="100%" height="68" style="border:0"></iframe>
```

Un outil qui demande une adresse et une hauteur, comme le bloc iframe d'un wiki, prend l'**Adresse** et 68 px.

Dans Notion, tapez `/integrer`, choisissez **Intégrer**, collez l'**Adresse** et validez par **Intégrer le lien**. Notion garde ses blocs plus hauts que le widget, qui reste en haut de son bloc. Choisissez **Selon l'appareil** avant de copier, pour que le widget suive le mode sombre de Notion.

Seul le widget peut s'afficher dans un autre site : toutes les autres pages de Statup refusent de s'afficher dans un cadre.

> [!NOTE]
> Certains intranets n'affichent un cadre que depuis une adresse que leur administrateur a autorisée. Si le widget reste vide, demandez-lui d'autoriser l'adresse de Statup.

Un intranet sur Internet, comme Notion, avec Statup sur le réseau de l'entreprise :

- Donnez à Statup une adresse en `https` : une page servie en `https` refuse un cadre en `http`.
- Chrome et Edge traitent le widget comme une requête vers le réseau local. Ils peuvent demander à chaque lecteur de l'autoriser, et le widget reste vide tant qu'il ne l'a pas fait. Un administrateur l'autorise pour tous les lecteurs avec la stratégie [`LocalNetworkAccessAllowedForUrls`](https://chromeenterprise.google/policies/local-network-access-allowed-for-urls/), réglée sur l'adresse de l'intranet.

## Le lien privé

L'adresse et le code portent toujours le lien privé de l'instance, la `key` de l'adresse. Avec lui, le widget montre ce que voit n'importe quel membre : les services ouverts à tous et aux membres, jamais un service restreint à des groupes.

Sans le lien, ou avec un ancien, le widget montre ce que voit un visiteur. Sur une page réglée sur **Membres seulement**, il n'affiche alors aucun état : seulement **État des services** et **Lien du widget à recopier depuis Statup**.

L'intranet où le widget est collé est en général derrière la connexion de l'entreprise, donc le lien reste entre collègues. S'il fuit, cliquez sur **Changer** sous **Changer le lien** et confirmez : l'ancien lien cesse aussitôt de fonctionner partout où il est collé, et une ligne s'ajoute au [Journal](journal.md). Recollez la nouvelle adresse ou le nouveau code là où se trouve le widget.

## Thème et langue

Le thème fait partie de l'adresse : `theme=dark` pour **Sombre**, `theme=auto` pour **Selon l'appareil**, rien pour **Clair**.

L'adresse porte aussi la langue de Statup au moment de la copie, `lang=fr` par exemple, et le widget la parle dans tous les outils. Pour le coller dans une autre langue, passez Statup dans cette langue avant de copier. Sans `lang`, le widget parle la langue que demande le navigateur du lecteur.

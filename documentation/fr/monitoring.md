---
title: Surveillance
description: "Laisser Statup vérifier qu'un site web ou un serveur répond, et afficher de lui-même une panne quand il ne répond plus."
---

Statup peut vérifier lui-même un service, chaque minute, depuis le serveur sur lequel il tourne : les adresses internes fonctionnent donc. Un outil de supervision que vous utilisez déjà peut aussi signaler les pannes, voir [Outils de supervision](monitoring-tools.md).

## Mettre en place une vérification

Dans le formulaire du service, sous **Surveillance**, choisissez :

- **Aucune**, par défaut : pas de vérification.
- **Web**, pour une adresse qui doit répondre. **URL** est une adresse complète en `http://` ou `https://`, comme `https://intranet.example.com`, de 2 000 caractères au plus. Cochez **Accepter un certificat interne** pour un outil dont le certificat n'est signé par aucune autorité publique.
- **Port**, pour un hôte et son port, comme `192.168.1.1` et `443` : un serveur, ou un routeur ou un pare-feu par son port d'administration. **Port** est un nombre de 1 à 65535.

Une adresse qui contient un identifiant ou un mot de passe est refusée : les vérifications ne conservent aucun identifiant de connexion.

**Tester** lance la vérification tout de suite, sans enregistrer, et dit pourquoi elle échoue : port fermé, aucune réponse en 10 secondes, nom inconnu, aucun serveur de noms qui répond, pas de réseau, certificat refusé, serveur qui répond par une erreur, adresse qui redirige en boucle, ou connexion qui échoue.

## Déroulement d'une vérification

- Chaque minute, jusqu'à 16 vérifications à la fois, chacune disposant de 10 secondes.
- **Web** envoie un `GET` avec le user agent `statup` et suit jusqu'à 5 redirections. Tout code inférieur à 500 compte comme une réponse, même un refus comme 401 ou 404.
- **Port** ouvre une connexion TCP et n'envoie rien. Une connexion acceptée vaut réponse.
- Les vérifications ne contactent que les adresses que vous avez réglées.

## Des échecs à la panne

Quand un service échoue à autant de vérifications de suite que son délai compte de minutes, trois par défaut, il passe **En panne**, marqué **Détecté automatiquement**, à compter de sa première vérification ratée. La première vérification qui obtient une réponse y met fin, et l'état réglé à la main revient.

Chaque service règle son délai sous **Surveillance** : « Déclarer une panne après 3 min sans réponse », de 1 à 60. Un serveur critique peut afficher sa panne au bout d'une minute, une imprimante au bout de quinze. Changer le délai, ou enregistrer le formulaire avec la même vérification, laisse telle quelle une panne déjà affichée.

Statup ne rédige jamais d'incident lui-même. **Déclarer un incident**, sur le bandeau ou sur la page Services, ouvre un nouvel incident avec l'impact **Majeur**, le service et le début déjà remplis. Voir [Incidents](incidents.md).

## Quand aucune panne n'est déclarée

- **Pendant une maintenance en cours** qui cite le service, sauf si elle est marquée **Sans interruption**. Les échecs sont comptés mais ne déclenchent rien. Si le service échoue encore à la fin de la maintenance, la panne est déclarée à la vérification ratée suivante, et datée de ce moment. Une vérification qui obtient une réponse pendant la maintenance met tout de même fin à une panne.
- **Quand Statup n'a plus de réseau.** Si tous les services vérifiés lors d'un passage échouent, au moins trois, tous parce que Statup n'a pas de route vers eux ou qu'aucun serveur de noms ne répond, les vérifications se mettent en pause aussi longtemps que cela dure. Le Journal note une ligne quand la pause commence et une quand le réseau revient. Si les services de la page dépendent tous du même serveur de noms et qu'il tombe, Statup ne peut pas le distinguer de sa propre coupure et se met aussi en pause.
- **Quand Statup perd sa connexion d'une autre façon.** Si au moins trois services qui répondaient échouent tous lors du même passage, pour une autre raison, ce passage est ignoré, pour cinq passages de suite au plus. Deux services qui échouent ensemble sont jugés normalement.

## Alertes

Les services qui tombent dans la même minute, comme ceux d'un serveur en cours de redémarrage, sont signalés en un seul message par canal (« 3 services ne répondent plus », avec leurs noms), et leur retour en un message de plus. Un service déjà couvert par un incident ouvert n'envoie aucune alerte. Déclarer une maintenance avant un redémarrage prévu fait taire les vérifications. Voir [Alertes](alerts.md).

## Dans l'historique

Une panne détectée de quinze minutes ou plus colore ses journées dans l'historique de disponibilité du service, sauf si un incident couvre déjà ce jour-là, et la journée affiche « Sans réponse pendant » suivi de la durée. Une panne plus courte laisse la journée en vert. Voir [Services](services.md#availability-history).

Sur ordinateur, un clic sur l'une de ces journées, sur le tableau de bord ou dans le panneau latéral d'un service, montre les pannes de ce jour sous **Pannes détectées** : quand le service a cessé de répondre et pendant combien de temps. Les membres lisent aussi pourquoi, dans les mots de **Tester**, ou qu'un outil de supervision l'a signalée ; les administrateurs ont un lien **Journal** vers les lignes de ces jours-là. Les pannes enregistrées avant la version 0.9.0 n'ont pas de raison.

**Ce n'était pas une panne**, pour les éditeurs et les administrateurs, efface une panne terminée. La case cochée dessous efface avec elle les pannes terminées des autres services qui ont commencé à cinq minutes près, comme quand Statup ou son serveur perd le réseau. Les journées repassent en vert, et le Journal note une ligne par service. Une panne en cours ne s'efface pas.

Changer le type de vérification, l'adresse, ou **Accepter un certificat interne** efface une panne en cours, par exemple une fausse panne due à une adresse erronée. Les pannes déjà terminées restent dans l'historique jusqu'à ce qu'on les efface.

## Couper les vérifications

[`MONITORING=false`](configuration.md#monitoring) arrête les vérifications lancées chaque minute. Les adresses restent enregistrées, et **Tester** vérifie toujours une adresse quand vous appuyez dessus.

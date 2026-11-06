---
title: Mot de passe oublié
description: "Comment un membre retrouve l'accès, par e-mail ou par un administrateur, et comment réinitialiser n'importe quel compte depuis le serveur."
---

## Par e-mail

Avec un serveur SMTP configuré ([`SMTP_HOST`](configuration.md#smtp_host) et les autres) et l'URL de la page connue ([`PUBLIC_URL`](configuration.md#public_url), ou **URL de la page** en bas de la page [Alertes](alerts.md)), les membres réinitialisent eux-mêmes leur mot de passe : **Mot de passe oublié ?** sur la page de connexion leur envoie un lien par e-mail.

- Le lien ne sert qu'une fois, pendant une heure.
- Choisir un nouveau mot de passe connecte le membre et ferme toutes les autres sessions du compte.
- La page répond de la même façon que l'adresse ait un compte ou non, et un compte reçoit au plus trois liens par heure.

## Par un administrateur

Sans serveur SMTP, ou tant que l'URL de la page est inconnue, la même page invite le membre à s'adresser à un administrateur, qui lui donne un mot de passe provisoire depuis la page [Équipe](team.md). Son lien **Réinitialiser depuis le serveur** mène à la partie suivante.

## Depuis le serveur

Si vous hébergez Statup, vous pouvez donner un mot de passe provisoire à n'importe quel compte actif, même quand aucun administrateur ne peut se connecter :

```bash
docker compose exec statup /app/statup reset-password you@example.com
```

Transmettez-le dans les sept jours. La personne s'en sert pour se connecter, puis doit choisir le sien, et toute session encore ouverte sur ce compte est fermée. Voir [Ligne de commande](command-line.md).

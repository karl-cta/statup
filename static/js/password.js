// Password fields: a button that shows what is typed, the password rule
// ticked as the author types, and the current password asked for a new email.
(function () {
    "use strict";

    document.querySelectorAll("[data-password-toggle]").forEach((button) => {
        const input = button.closest(".password-field").querySelector("input");
        button.hidden = false;
        button.addEventListener("click", () => {
            const shown = input.type === "password";
            input.type = shown ? "text" : "password";
            button.setAttribute("aria-pressed", String(shown));
        });
        // A password manager must not file the text as a plain field.
        input.form.addEventListener("submit", () => {
            input.type = "password";
        });
    });

    // The server's rule: the field's minimum length with lowercase,
    // uppercase, digits and symbols in it, or a passphrase of any kind.
    const PASSPHRASE_LENGTH = 20;
    const KINDS = [/\p{Lowercase}/u, /\p{Uppercase}/u, /\p{N}/u, /[^\p{Alphabetic}\p{N}]/u];

    document.querySelectorAll("[data-password-rule]").forEach((rule) => {
        const input = rule.closest(".field").querySelector("input");
        const check = () => {
            const length = [...input.value].length;
            const mixed = length >= input.minLength && KINDS.every((kind) => kind.test(input.value));
            rule.dataset.met = String(length >= PASSPHRASE_LENGTH || mixed);
        };
        input.addEventListener("input", check);
        check();
    });

    // A new email asks for the current password, as the server does.
    document.querySelectorAll("[data-saved-email]").forEach((input) => {
        const field = document.getElementById(input.dataset.confirmField);
        const password = field.querySelector("input");
        const sync = () => {
            const changed = input.value.trim().toLowerCase() !== input.dataset.savedEmail;
            field.hidden = !changed;
            password.required = changed;
        };
        input.addEventListener("input", sync);
        sync();
    });
})();

// Password fields: a button that shows what is typed, and the password rule
// ticked as the author types.
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
})();

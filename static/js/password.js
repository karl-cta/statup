// Password fields: a button that shows what is typed, and the length rule
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

    document.querySelectorAll("[data-password-rule]").forEach((rule) => {
        const input = rule.closest(".field").querySelector("input");
        const check = () => {
            rule.dataset.met = String([...input.value].length >= input.minLength);
        };
        input.addEventListener("input", check);
        check();
    });
})();

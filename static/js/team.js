// A role change asks in the row, naming the person and what the role allows,
// before anything is posted.
(function () {
    "use strict";

    const selector = '[data-role-form] select[name="role"]';

    function restore(select) {
        select.value = select.dataset.committed;
        select.dispatchEvent(new CustomEvent("cs:sync"));
    }

    // The select carries one sentence per role: "questionAdmin", "descReader".
    function roleText(select, kind) {
        const role = select.value;
        return select.dataset[`${kind}${role.charAt(0).toUpperCase()}${role.slice(1)}`] || "";
    }

    document.addEventListener("change", (event) => {
        const select = event.target;
        if (!(select instanceof HTMLSelectElement) || !select.matches(selector)) return;
        const form = select.closest("[data-role-form]");
        const box = form.querySelector("[data-role-confirm]");
        if (select.value === select.dataset.committed) {
            box.hidden = true;
            return;
        }
        const question = box.querySelector("[data-role-question]");
        question.textContent = roleText(select, "question").replace("{name}", question.dataset.name);
        box.querySelector("[data-role-desc]").textContent = roleText(select, "desc");
        box.hidden = false;
        box.querySelector("[data-role-cancel]").focus();
    });

    document.addEventListener("click", (event) => {
        const cancel = event.target instanceof Element ? event.target.closest("[data-role-cancel]") : null;
        if (!cancel) return;
        const form = cancel.closest("[data-role-form]");
        form.querySelector("[data-role-confirm]").hidden = true;
        const select = form.querySelector(selector);
        restore(select);
        const trigger = form.querySelector(".cs-trigger");
        (trigger || select).focus();
    });
})();

// Monitoring fields, on a service's page and at the first launch: the
// address row follows the chosen kind, and the row that does not apply leaves
// the request. A test result belongs to the check it was run for, so changing
// the check clears it.
(function () {
    "use strict";

    const form = document.querySelector("[data-check-fields]");
    if (!form) return;

    // A row opened by a choice slides in; one open on arrival does not.
    form.addEventListener("animationend", (event) => event.target.classList.remove("is-revealing"));

    function setOpen(row, open, animate) {
        if (animate && open && !row.classList.contains("is-open")) row.classList.add("is-revealing");
        row.classList.toggle("is-open", open);
        row.querySelectorAll("input").forEach((field) => {
            field.disabled = !open;
            if (field.hasAttribute("data-required")) field.required = open;
        });
    }

    function update(animate) {
        const chosen = form.querySelector('input[name="check_kind"]:checked');
        const kind = chosen ? chosen.value : "none";
        form.querySelectorAll("[data-for-check]").forEach((row) => setOpen(row, row.dataset.forCheck === kind, animate));
    }

    form.addEventListener("change", (event) => {
        if (event.target.name === "check_kind") update(true);
        if (event.target.closest("[data-for-check]") || event.target.name === "check_kind") {
            form.querySelectorAll("[data-check-result]").forEach((result) => result.replaceChildren());
        }
    });
    update(false);
})();

// The service status control publishes to every visitor: an outage level
// asks first, and a refused change puts the menu back on the saved value.
(function () {
    "use strict";

    const selector = '[data-status-form] select[name="status"]';

    function cellOf(element) {
        return element.closest(".status-cell");
    }

    function restore(select) {
        select.value = select.dataset.committed;
        select.dispatchEvent(new CustomEvent("cs:sync"));
    }

    function ask(cell, select) {
        const box = cell.querySelector("[data-status-confirm]");
        const question = box.querySelector("[data-status-question]");
        const label = select.options[select.selectedIndex].textContent.trim();
        question.textContent = question.dataset.template.replace("{status}", label);
        box.hidden = false;
        box.querySelector("[data-status-cancel]").focus();
    }

    function publish(cell) {
        cell.querySelector("[data-status-confirm]").hidden = true;
        cell.querySelector("[data-status-form]").requestSubmit();
    }

    document.addEventListener("change", (event) => {
        const select = event.target;
        if (!(select instanceof HTMLSelectElement) || !select.matches(selector)) return;
        const cell = cellOf(select);
        if (select.value === select.dataset.committed) {
            cell.querySelector("[data-status-confirm]").hidden = true;
            return;
        }
        if (select.selectedOptions[0].hasAttribute("data-guarded")) {
            ask(cell, select);
        } else {
            publish(cell);
        }
    });

    document.addEventListener("click", (event) => {
        const target = event.target;
        if (!(target instanceof Element)) return;
        const go = target.closest("[data-status-publish]");
        if (go) {
            publish(cellOf(go));
            return;
        }
        const cancel = target.closest("[data-status-cancel]");
        if (!cancel) return;
        const cell = cellOf(cancel);
        cell.querySelector("[data-status-confirm]").hidden = true;
        restore(cell.querySelector(selector));
        const trigger = cell.querySelector(".cs-trigger");
        if (trigger) trigger.focus();
    });

    // The cell is replaced once the server answers: the keyboard carries on
    // from the receipt's undo, or from the menu once a change is undone.
    let refocus = false;

    document.body.addEventListener("htmx:beforeRequest", (event) => {
        const elt = event.detail.elt;
        if (elt instanceof Element && elt.closest(".status-cell")) refocus = true;
    });

    document.body.addEventListener("htmx:afterSettle", (event) => {
        const cell = event.target;
        if (!refocus || !(cell instanceof Element) || !cell.matches(".status-cell")) return;
        refocus = false;
        const next = cell.querySelector(".receipt-undo") || cell.querySelector(".cs-trigger");
        if (next) next.focus();
    });

    document.body.addEventListener("htmx:afterRequest", (event) => {
        const elt = event.detail.elt;
        const cell = !event.detail.successful && elt instanceof Element ? elt.closest(".status-cell") : null;
        if (!cell) return;
        refocus = false;
        const select = cell.querySelector(selector);
        if (select) restore(select);
        const trigger = cell.querySelector(".cs-trigger");
        if (trigger) trigger.focus();
    });
})();

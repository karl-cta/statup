// Who sees a service or a folder. Every group and member has its tag in
// the page, hidden until granted: adding from the panel shows it, its cross
// hides it again, so the form sends what the tags show. Inheriting from the
// folder freezes the rest; the sentence under it is redrawn on each change.
(function () {
    "use strict";

    const tagOf = (field, key) => field.querySelector(`[data-grant="${key}"]`);
    const rowOf = (field, key) => field.querySelector(`[data-grant-choice="${key}"]`);

    function reached(field) {
        const readers = new Set();
        for (const input of field.querySelectorAll("[data-grant] input:not(:disabled)")) {
            for (const id of input.dataset.readers.split(",")) {
                if (id) readers.add(id);
            }
        }
        return readers.size;
    }

    function redraw(field) {
        const texts = field.dataset;
        const mode = field.querySelector('input[name="visibility"]:checked')?.value;
        let sentence = texts.effectStaff;
        if (mode === "everyone") {
            sentence = texts.public === "true" ? texts.effectPublic : texts.effectAllMembers;
        } else if (mode === "members") {
            sentence = texts.effectMembers;
        } else {
            const count = reached(field);
            if (count > 0) {
                sentence = (count === 1 ? texts.effectOne : texts.effectOther).replace("{n}", String(count));
            }
        }
        field.querySelector("[data-access-effect]").textContent = sentence;
    }

    function setGranted(field, key, granted) {
        const tag = tagOf(field, key);
        const row = rowOf(field, key);
        if (!tag || !row) return;
        tag.hidden = !granted;
        tag.classList.remove("is-extra");
        tag.querySelector("input").disabled = !granted;
        row.hidden = granted;
        row.querySelector("input").checked = false;
        if (granted) {
            row.removeAttribute("data-filter-text");
        } else {
            row.dataset.filterText = row.dataset.choiceText;
        }
    }

    function addTicked(field, button) {
        const panel = button.closest("details");
        for (const box of field.querySelectorAll("[data-grant-choice] input:checked")) {
            setGranted(field, box.closest("[data-grant-choice]").dataset.grantChoice, true);
        }
        field.querySelector("[data-grant-more]")?.remove();
        for (const tag of field.querySelectorAll(".grant.is-extra")) tag.classList.remove("is-extra");
        if (panel) {
            panel.open = false;
            panel.querySelector("summary").focus();
        }
        redraw(field);
    }

    // The removed tag hides its own button: focus goes to the next tag
    // still shown, else to "Add".
    function focusAfterRemove(field, tag) {
        const shown = [...field.querySelectorAll("[data-grant]")].filter((other) => !other.hidden && other.getClientRects().length > 0);
        const next = shown.find((other) => tag.compareDocumentPosition(other) & Node.DOCUMENT_POSITION_FOLLOWING) || shown.pop();
        const target = next ? next.querySelector("[data-grant-remove]") : field.querySelector(".grant-add > summary");
        if (target) target.focus();
    }

    document.addEventListener("click", (event) => {
        const target = event.target;
        if (!(target instanceof Element)) return;
        const field = target.closest("[data-access-field]");
        if (!field) return;
        const remove = target.closest("[data-grant-remove]");
        if (remove) {
            const tag = remove.closest("[data-grant]");
            setGranted(field, tag.dataset.grant, false);
            redraw(field);
            focusAfterRemove(field, tag);
            return;
        }
        const add = target.closest("[data-grant-add]");
        if (add) {
            addTicked(field, add);
            return;
        }
        if (target.closest("[data-grant-more]")) {
            const extra = [...field.querySelectorAll(".grant.is-extra")];
            for (const tag of extra) tag.classList.remove("is-extra");
            target.closest("[data-grant-more]").remove();
            const first = extra[0] && extra[0].querySelector("[data-grant-remove]");
            if (first) first.focus();
        }
    });

    document.addEventListener("change", (event) => {
        const input = event.target;
        if (!(input instanceof HTMLInputElement)) return;
        const field = input.closest("[data-access-field]");
        if (!field) return;
        if (input.matches("[data-access-inherit]")) {
            field.querySelector("[data-access-body]").inert = input.checked;
        }
        if (input.name === "visibility") redraw(field);
    });
})();

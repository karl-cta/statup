// Icon picker of the service form. The chosen cell is the one whose
// aria-checked is true; the stylesheet draws the choice from it.
(function () {
    "use strict";

    const picker = document.querySelector("[data-icon-picker]");
    if (!picker) return;

    const nameInput = picker.querySelector("[data-icon-name]");
    const idInput = picker.querySelector("[data-icon-id]");
    const preview = picker.querySelector("[data-icon-preview]");
    const choices = picker.querySelector("[data-icon-choices]");
    const label = picker.querySelector("[data-icon-choices-label]");
    const clear = picker.querySelector("[data-icon-clear]");
    const placeholder = picker.querySelector("[data-icon-placeholder]");
    const current = picker.querySelector("[data-icon-current]");
    const summary = choices.querySelector("summary");

    const cells = () => Array.from(picker.querySelectorAll(".icon-cell"));

    // One tab stop per group: the chosen cell, or the first one.
    function roving() {
        picker.querySelectorAll('[role="radiogroup"]').forEach((group) => {
            const groupCells = Array.from(group.querySelectorAll(".icon-cell"));
            const chosen = groupCells.find((cell) => cell.getAttribute("aria-checked") === "true") || groupCells[0];
            groupCells.forEach((cell) => {
                cell.tabIndex = cell === chosen ? 0 : -1;
            });
        });
    }

    // A cell's name shows beside its group title while it is hovered or
    // focused; the chosen one keeps it.
    function echo(group, cell) {
        const target = group.querySelector("[data-icon-echo]");
        if (target) target.textContent = cell ? cell.dataset.iconLabel : "";
    }

    function restoreEchoes() {
        picker.querySelectorAll("[data-icon-group]").forEach((group) => {
            echo(group, group.querySelector('.icon-cell[aria-checked="true"]'));
        });
    }

    function markChosen(chosen) {
        cells().forEach((cell) => cell.setAttribute("aria-checked", String(cell === chosen)));
        roving();
        restoreEchoes();
    }

    function showPreview(node, empty) {
        preview.replaceChildren(node);
        preview.classList.toggle("icon-preview-empty", empty);
        label.textContent = empty ? label.dataset.choose : label.dataset.change;
        clear.classList.toggle("invisible", empty);
    }

    // The summary names the chosen icon for screen readers: its picture
    // says nothing to them.
    function sayChosen(cell) {
        if (!current) return;
        current.textContent = cell ? current.dataset.template.replace("{name}", cell.dataset.iconLabel || "") : "";
    }

    function choose(cell) {
        // A cell whose image failed to load has nothing to show.
        const drawing = cell.querySelector("svg, img");
        if (!drawing) return;
        if (cell.dataset.iconBuiltin) {
            nameInput.value = cell.dataset.iconBuiltin;
            idInput.value = "";
        } else {
            idInput.value = cell.dataset.iconCustom;
            nameInput.value = "";
        }
        const copy = drawing.cloneNode(true);
        if (copy instanceof HTMLImageElement) copy.removeAttribute("loading");
        showPreview(copy, false);
        markChosen(cell);
        sayChosen(cell);
    }

    // The remove button hides itself: the keyboard goes on from the summary.
    // The choices slide in when someone opens them, not on arrival.
    const body = choices.querySelector(".icon-choices-body");

    function reveal() {
        if (!choices.open) body.classList.add("is-revealing");
    }

    body.addEventListener("animationend", () => body.classList.remove("is-revealing"));

    function reset() {
        nameInput.value = "";
        idInput.value = "";
        showPreview(placeholder.content.cloneNode(true), true);
        markChosen(null);
        sayChosen(null);
        reveal();
        choices.open = true;
        summary.focus();
    }

    picker.addEventListener("click", (event) => {
        const cell = event.target.closest(".icon-cell");
        if (cell) choose(cell);
        else if (event.target.closest("[data-icon-clear]")) reset();
        else if (event.target.closest("summary")) reveal();
    });

    picker.addEventListener("keydown", (event) => {
        const cell = event.target.closest(".icon-cell");
        if (!cell) return;
        const steps = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };
        if (!(event.key in steps) && event.key !== "Home" && event.key !== "End") return;
        event.preventDefault();
        const group = Array.from(cell.closest('[role="radiogroup"]').querySelectorAll(".icon-cell"));
        const index = group.indexOf(cell);
        let next = (index + (steps[event.key] || 0) + group.length) % group.length;
        if (event.key === "Home") next = 0;
        if (event.key === "End") next = group.length - 1;
        group[next].focus();
        choose(group[next]);
    });

    ["pointerover", "focusin"].forEach((type) => {
        picker.addEventListener(type, (event) => {
            const cell = event.target.closest(".icon-cell");
            if (cell) echo(cell.closest("[data-icon-group]"), cell);
        });
    });

    ["pointerout", "focusout"].forEach((type) => {
        picker.addEventListener(type, (event) => {
            const grid = event.target.closest('[role="radiogroup"]');
            const into = event.relatedTarget instanceof Element ? event.relatedTarget.closest('[role="radiogroup"]') : null;
            if (grid && grid !== into) restoreEchoes();
        });
    });

    // An upload swaps the custom grid with the new icon chosen; the hidden
    // inputs and the preview live outside it. A refused file leaves the
    // choice as it was.
    document.body.addEventListener("htmx:afterSwap", (event) => {
        const grid = event.target;
        if (!(grid instanceof Element) || grid.id !== "custom-icons-grid") return;
        const added = grid.querySelector('.icon-cell[aria-checked="true"]');
        if (added) {
            choose(added);
            return;
        }
        const kept = idInput.value ? grid.querySelector(`[data-icon-custom="${CSS.escape(idInput.value)}"]`) : null;
        if (kept) kept.setAttribute("aria-checked", "true");
        roving();
        restoreEchoes();
    });

    // Clears the file input once answered, so the same file can be sent again.
    document.body.addEventListener("htmx:afterRequest", (event) => {
        const input = event.detail.elt;
        if (input instanceof HTMLInputElement && input.hasAttribute("data-icon-upload")) input.value = "";
    });

    roving();
    restoreEchoes();
})();

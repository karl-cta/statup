// First launch: the preview on the right follows what is typed and ticked
// before it is saved, and the last step lights the banner once.
(function () {
    "use strict";

    const sheet = document.querySelector("[data-sheet]");
    if (!sheet) return;

    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    // Restarts a one-shot animation on an element that may already carry it.
    function replay(element, className) {
        element.classList.remove(className);
        void element.offsetWidth;
        element.classList.add(className);
    }

    // The page step: name, logo and audience.
    const nameField = document.querySelector("[data-setup-name]");
    const sheetName = sheet.querySelector("[data-sheet-name]");
    const productName = sheetName.innerHTML;
    if (nameField) {
        nameField.addEventListener("input", () => {
            const name = nameField.value.trim();
            if (name) sheetName.textContent = name;
            else sheetName.innerHTML = productName;
        });
    }

    const logoField = document.querySelector("[data-setup-logo]");
    if (logoField) {
        logoField.addEventListener("change", () => {
            const file = logoField.files[0];
            if (!file) return;
            const url = URL.createObjectURL(file);
            const thumb = document.querySelector("[data-setup-logo-thumb]");
            const image = document.createElement("img");
            image.src = url;
            image.alt = "";
            thumb.replaceChildren(image);
            document.querySelector("[data-setup-logo-cta]").textContent = file.name;
            let logo = sheet.querySelector("[data-sheet-logo]");
            if (!logo) {
                logo = document.createElement("img");
                logo.className = "sheet-logo";
                logo.alt = "";
                logo.dataset.sheetLogo = "";
                sheet.querySelector("[data-sheet-name]").before(logo);
            }
            logo.src = url;
            const mark = sheet.querySelector("[data-sheet-mark]");
            if (mark) mark.hidden = true;
            replay(logo, "sheet-swap");
        });
    }

    const access = sheet.querySelector("[data-sheet-access]");
    document.querySelectorAll("[data-setup-access]").forEach((radio) => {
        radio.addEventListener("change", () => {
            access.textContent = radio.value === "everyone" ? access.dataset.open : access.dataset.closed;
            replay(access, "sheet-swap");
        });
    });

    // The services step: a ticked service slides into the page, its thirty
    // days drawing in; the rows already there glide to their new place.
    const list = sheet.querySelector("[data-sheet-list]");
    const empty = sheet.querySelector("[data-sheet-empty]");
    const banner = sheet.querySelector("[data-sheet-banner]");
    const ring = sheet.querySelector("[data-sheet-ring]");
    const PREVIEW_ROWS = 6;
    const GENERIC_ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21.75 17.25v-.228a4.5 4.5 0 0 0-.12-1.03l-2.268-9.64a3.375 3.375 0 0 0-3.285-2.602H7.923a3.375 3.375 0 0 0-3.285 2.602l-2.268 9.64a4.5 4.5 0 0 0-.12 1.03v.228m19.5 0a3 3 0 0 1-3 3H5.25a3 3 0 0 1-3-3m19.5 0a3 3 0 0 0-3-3H5.25a3 3 0 0 0-3 3m16.5 0h.008v.008h-.008v-.008Zm-3 0h.008v.008h-.008v-.008Z"/></svg>';

    function rows() {
        return Array.from(list.querySelectorAll(".sheet-svc:not(.is-gone)"));
    }

    function glide(change) {
        const before = new Map(Array.from(list.children, (row) => [row, row.getBoundingClientRect().top]));
        change();
        if (reduced) return;
        list.querySelectorAll(".sheet-svc:not(.is-new)").forEach((row) => {
            const top = before.get(row);
            if (top === undefined) return;
            const shift = top - row.getBoundingClientRect().top;
            if (shift) {
                row.animate([{ transform: `translateY(${shift}px)` }, { transform: "none" }], {
                    duration: 380,
                    easing: "cubic-bezier(0.16, 1, 0.3, 1)",
                });
            }
        });
    }

    function makeRow(name, icon) {
        const row = document.createElement("li");
        row.className = "sheet-svc is-new";
        row.dataset.name = name;
        const label = document.createElement("span");
        label.className = "sheet-svc-name";
        const iconHolder = document.createElement("span");
        iconHolder.className = "sheet-svc-icon";
        if (icon) {
            const drawing = icon.cloneNode(true);
            drawing.removeAttribute("hidden");
            drawing.removeAttribute("class");
            iconHolder.append(drawing);
        } else {
            iconHolder.innerHTML = GENERIC_ICON;
        }
        label.append(iconHolder, name);
        const state = document.createElement("span");
        state.className = "sheet-svc-state";
        state.textContent = list.dataset.state;
        const bars = document.createElement("span");
        bars.className = "sheet-bars";
        bars.setAttribute("aria-hidden", "true");
        bars.innerHTML = "<i></i>".repeat(29) + '<i class="is-on"></i>';
        row.append(label, state, bars);
        row.addEventListener("animationend", (event) => {
            if (event.target === row) row.classList.remove("is-new");
        });
        return row;
    }

    // The banner and the count past the rows drawn follow the ticked list.
    function paint(total) {
        empty.hidden = total > 0;
        const key = total === 0 ? "none" : total === 1 ? "one" : "all";
        if (banner.textContent !== banner.dataset[key]) {
            banner.textContent = banner.dataset[key];
            replay(banner, "sheet-swap");
        }
        ring.classList.toggle("is-quiet", total === 0);
        let more = list.querySelector("[data-sheet-more]");
        const beyond = more ? Number(more.dataset.count) : 0;
        const hidden = Math.max(rows().length - PREVIEW_ROWS, 0) + beyond;
        rows().forEach((row, index) => {
            row.hidden = index >= PREVIEW_ROWS;
        });
        if (hidden === 0) {
            if (more) more.remove();
            return;
        }
        if (!more) {
            more = document.createElement("li");
            more.className = "sheet-more";
            more.dataset.sheetMore = "";
            more.dataset.count = "0";
        }
        more.textContent = list.dataset.more.replace("{n}", String(hidden));
        list.append(more);
    }

    function tick(box) {
        const name = box.value;
        const chip = box.closest(".setup-chip");
        if (box.checked) {
            glide(() => list.prepend(makeRow(name, chip.querySelector(".setup-chip-icon"))));
        } else {
            const row = rows().find((r) => r.dataset.name === name);
            if (row) {
                row.classList.add("is-gone");
                const drop = () => glide(() => row.remove());
                if (reduced) drop();
                else row.addEventListener("animationend", drop, { once: true });
            }
        }
        replay(chip, "is-bump");
        paint(rows().length);
    }

    const chips = document.querySelector("[data-setup-chips]");
    if (chips) {
        chips.addEventListener("change", (event) => {
            if (event.target instanceof HTMLInputElement) tick(event.target);
        });
    }

    // A name typed freely becomes a ticked chip of its own; without the
    // script the field is sent as is.
    const custom = document.querySelector("[data-setup-custom]");
    const addButton = document.querySelector("[data-setup-add]");
    function addTyped() {
        const name = custom.value.trim();
        custom.value = "";
        if (!name) return;
        const known = Array.from(chips.querySelectorAll("input")).find(
            (box) => box.value.toLowerCase() === name.toLowerCase(),
        );
        if (known) {
            if (!known.checked && !known.disabled) {
                known.checked = true;
                tick(known);
            }
            return;
        }
        const chip = chips.querySelector(".setup-chip").cloneNode(true);
        chip.querySelector(".setup-chip-icon").remove();
        const box = chip.querySelector("input");
        box.value = name;
        box.checked = true;
        box.disabled = false;
        chip.querySelector("span").textContent = name;
        chips.append(chip);
        tick(box);
    }
    if (custom && addButton && chips) {
        addButton.hidden = false;
        addButton.addEventListener("click", () => {
            addTyped();
            custom.focus();
        });
        custom.addEventListener("keydown", (event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            addTyped();
        });
    }

    // The last step: the page is ready, its ring lights once.
    if (document.querySelector("[data-setup-done]") && !ring.classList.contains("is-quiet")) {
        ring.classList.add("is-live");
    }
})();

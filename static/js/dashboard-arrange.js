// Arranging the dashboard on the dashboard itself. An administrator opens
// the mode, drags a block by its handle (any pointer) or moves it with the
// arrow keys, picks its width, hides it or adds a hidden one back. Every
// change shows at once and is saved in the background; a refused save
// reloads the page, the only way back to a truthful screen.
(function () {
    "use strict";

    const KEY_SAVE_DELAY = 600;
    const page = document.querySelector("[data-arrange]");
    const live = document.getElementById("live");
    const toggle = document.querySelector("[data-arrange-toggle]");
    if (!page || !live || !toggle) return;

    let drag = null;
    let saveTimer = null;

    const csrf = () => {
        const meta = document.querySelector('meta[name="csrf-token"]');
        return meta ? meta.content : "";
    };

    function announce(text) {
        const region = document.getElementById("announcer");
        if (!region || !text) return;
        region.textContent = "";
        window.setTimeout(() => {
            region.textContent = text;
        }, 100);
    }

    function post(path, fields) {
        return fetch(path, {
            method: "POST",
            body: new URLSearchParams(fields),
            credentials: "same-origin",
            headers: { "X-CSRF-Token": csrf() },
            keepalive: true,
        }).then((response) => {
            if (!response.ok) throw new Error(`not saved: ${response.status}`);
        });
    }

    const reload = () => window.location.reload();

    function arranging() {
        return document.body.classList.contains("is-arranging");
    }

    function reveal(on) {
        document.querySelectorAll("[data-arrange-only]").forEach((element) => {
            element.hidden = !on;
        });
    }

    function setMode(on) {
        document.body.classList.toggle("is-arranging", on);
        reveal(on);
        toggle.setAttribute("aria-pressed", String(on));
        toggle.textContent = on ? toggle.dataset.labelClose : toggle.dataset.labelOpen;
        const url = new URL(window.location.href);
        if (on) url.searchParams.set("arrange", "1");
        else url.searchParams.delete("arrange");
        window.history.replaceState(null, "", url);
    }

    function cells() {
        return Array.from(live.querySelectorAll(".dash-cell[data-module-id]"));
    }

    function orderKey() {
        return cells().map((cell) => cell.dataset.moduleId).join(",");
    }

    function saveOrder() {
        window.clearTimeout(saveTimer);
        saveTimer = null;
        const fields = cells().map((cell) => ["order", cell.dataset.moduleId]);
        post("/admin/dashboard/layout/order", fields)
            .then(() => announce(page.dataset.saved))
            .catch(reload);
    }

    function announceMove(cell) {
        const all = cells();
        announce(
            (page.dataset.moved || "")
                .replace("{name}", cell.dataset.moduleName || "")
                .replace("{index}", String(all.indexOf(cell) + 1))
                .replace("{total}", String(all.length)),
        );
    }

    // The fragment is fetched again after a block is hidden or added, so
    // the page shows what the server will show; the mode survives the swap.
    function refetch() {
        if (!window.htmx) {
            reload();
            return;
        }
        window.htmx.ajax("GET", live.dataset.live, { target: live, swap: "innerHTML" }).catch(reload);
    }

    live.addEventListener("htmx:afterSettle", () => {
        if (arranging()) reveal(true);
    });

    function setWidth(cell, width) {
        cell.dataset.width = width;
        cell.querySelectorAll("[data-size]").forEach((button) => {
            button.setAttribute("aria-pressed", String(button.dataset.size === width));
        });
        post(`/admin/dashboard/layout/${cell.dataset.moduleId}/width`, [["width", width]])
            .then(() => announce(page.dataset.saved))
            .catch(reload);
    }

    function setShown(moduleId, shown) {
        post(`/admin/dashboard/layout/${moduleId}/toggle`, [["enabled", String(shown)]])
            .then(refetch)
            .catch(reload);
    }

    // Where the pointer is: before a block when it sits in the block's
    // first half, after it otherwise. A block on a row of its own is split
    // top and bottom, the others left and right.
    function placeAt(x, y) {
        const target = document.elementFromPoint(x, y);
        const cell = target instanceof Element ? target.closest(".dash-cell[data-module-id]") : null;
        if (!cell || cell === drag.cell) return;
        const box = cell.getBoundingClientRect();
        const vertical = cell.dataset.width === "full" || box.width > window.innerWidth * 0.8;
        const before = vertical ? y < box.top + box.height / 2 : x < box.left + box.width / 2;
        const anchor = before ? cell : cell.nextElementSibling;
        if (anchor === drag.cell || anchor === drag.cell.nextElementSibling) return;
        cell.parentElement.insertBefore(drag.cell, anchor);
    }

    live.addEventListener("pointerdown", (event) => {
        const handle = event.target.closest("[data-drag-handle]");
        if (!handle || drag || (event.pointerType === "mouse" && event.button !== 0)) return;
        event.preventDefault();
        const cell = handle.closest(".dash-cell[data-module-id]");
        handle.setPointerCapture(event.pointerId);
        drag = { cell, pointerId: event.pointerId, startOrder: orderKey() };
        cell.classList.add("is-dragging");
    });

    live.addEventListener("pointermove", (event) => {
        if (!drag || event.pointerId !== drag.pointerId) return;
        placeAt(event.clientX, event.clientY);
    });

    function endDrag(event) {
        if (!drag || event.pointerId !== drag.pointerId) return;
        const { cell, startOrder } = drag;
        drag = null;
        cell.classList.remove("is-dragging");
        if (orderKey() === startOrder) return;
        announceMove(cell);
        saveOrder();
    }

    live.addEventListener("pointerup", endDrag);
    live.addEventListener("pointercancel", endDrag);

    live.addEventListener("keydown", (event) => {
        const handle = event.target.closest("[data-drag-handle]");
        if (!handle || drag) return;
        const earlier = event.key === "ArrowLeft" || event.key === "ArrowUp";
        const later = event.key === "ArrowRight" || event.key === "ArrowDown";
        if (!earlier && !later) return;
        event.preventDefault();
        const cell = handle.closest(".dash-cell[data-module-id]");
        const sibling = earlier ? cell.previousElementSibling : cell.nextElementSibling;
        if (!sibling) return;
        cell.parentElement.insertBefore(cell, earlier ? sibling : sibling.nextElementSibling);
        handle.focus();
        announceMove(cell);
        window.clearTimeout(saveTimer);
        saveTimer = window.setTimeout(saveOrder, KEY_SAVE_DELAY);
    });

    live.addEventListener("click", (event) => {
        const size = event.target.closest("[data-size]");
        if (size) {
            setWidth(size.closest(".dash-cell[data-module-id]"), size.dataset.size);
            return;
        }
        const hide = event.target.closest("[data-hide]");
        if (hide) {
            setShown(hide.closest(".dash-cell[data-module-id]").dataset.moduleId, false);
            return;
        }
        const show = event.target.closest("[data-show]");
        if (show) setShown(show.dataset.show, true);
    });

    toggle.addEventListener("click", () => setMode(!arranging()));
    window.addEventListener("pagehide", () => {
        if (saveTimer) saveOrder();
    });

    if (page.hasAttribute("data-arrange-start")) setMode(true);
})();

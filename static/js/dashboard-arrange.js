// Arranging the dashboard on the dashboard itself. An administrator opens
// the mode, drags a block by its handle (any pointer) or moves it with the
// arrow keys, picks its width, hides it or adds a hidden one back. Every
// change shows at once and is saved in the background; a refused save
// reloads the page, the only way back to a truthful screen.
(function () {
    "use strict";

    const KEY_SAVE_DELAY = 600;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
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
        if (!on) closeOptions();
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

    // A card's settings: a tick saves at once and redraws the card, its
    // panel open again and the focus back on the same box.
    let openOptions = null;

    function setOptionsOpen(cell, open) {
        const panel = cell.querySelector("[data-options]");
        const button = cell.querySelector("[data-options-toggle]");
        if (!panel || !button) return;
        panel.hidden = !open;
        button.setAttribute("aria-expanded", String(open));
        openOptions = open ? { moduleId: cell.dataset.moduleId, value: openOptions?.value } : null;
    }

    function closeOptions() {
        if (!openOptions) return;
        const cell = live.querySelector(`.dash-cell[data-module-id="${openOptions.moduleId}"]`);
        if (cell) setOptionsOpen(cell, false);
        openOptions = null;
    }

    function saveShown(cell, box) {
        const values = Array.from(cell.querySelectorAll("[data-options] input:checked"), (input) => ["show", input.value]);
        openOptions = { moduleId: cell.dataset.moduleId, value: box.value };
        post(`/admin/dashboard/layout/${cell.dataset.moduleId}/show`, values)
            .then(refetch)
            .catch(reload);
    }

    live.addEventListener("change", (event) => {
        const box = event.target.closest("[data-options] input");
        if (!box) return;
        const cell = box.closest(".dash-cell[data-module-id]");
        // One stays ticked: a card that shows nothing is a blank card.
        if (!cell.querySelector("[data-options] input:checked")) {
            box.checked = true;
            return;
        }
        saveShown(cell, box);
    });

    live.addEventListener("htmx:afterSettle", () => {
        if (arranging()) reveal(true);
        if (!openOptions) return;
        const { moduleId, value } = openOptions;
        const cell = live.querySelector(`.dash-cell[data-module-id="${moduleId}"]`);
        if (!cell) return;
        setOptionsOpen(cell, true);
        const box = cell.querySelector(`[data-options] input[value="${value}"]`);
        if (box) box.focus();
    });

    document.addEventListener("click", (event) => {
        if (openOptions && event.target instanceof Element && !event.target.closest("[data-options], [data-options-toggle]")) {
            closeOptions();
        }
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

    // Dragging: the block lifts and follows the pointer, a dashed slot shows
    // where it will land, the other blocks glide out of its way, and on
    // release it settles into the slot. A press that does not move is a
    // click. Escape puts everything back.
    const DRAG_THRESHOLD = 4;
    const EDGE = 64;
    const REORDER_PAUSE = 140;
    let pending = null;
    let lastReorder = 0;
    let scrollSpeed = 0;
    let frame = 0;

    // Blocks already on the page glide from where they were to where the
    // change put them.
    function flip(change) {
        const before = new Map(cells().map((cell) => [cell, cell.getBoundingClientRect()]));
        change();
        if (reduced) return;
        cells().forEach((cell) => {
            const was = before.get(cell);
            const now = cell.getBoundingClientRect();
            const dx = was.left - now.left;
            const dy = was.top - now.top;
            if (dx || dy) {
                cell.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], {
                    duration: 240,
                    easing: "cubic-bezier(0.16, 1, 0.3, 1)",
                });
            }
        });
    }

    // Where the pointer is: before a block when it sits in the block's
    // first half, after it otherwise. A block on a row of its own is split
    // top and bottom, the others left and right.
    function placeAt(x, y) {
        if (performance.now() - lastReorder < REORDER_PAUSE) return;
        const target = document.elementFromPoint(x, y);
        const cell = target instanceof Element ? target.closest(".dash-cell[data-module-id]") : null;
        if (!cell || cell === drag.cell) return;
        const box = cell.getBoundingClientRect();
        const vertical = cell.dataset.width === "full" || box.width > window.innerWidth * 0.8;
        const before = vertical ? y < box.top + box.height / 2 : x < box.left + box.width / 2;
        const anchor = before ? cell : cell.nextElementSibling;
        if (anchor === drag.cell || anchor === drag.cell.nextElementSibling) return;
        lastReorder = performance.now();
        flip(() => cell.parentElement.insertBefore(drag.cell, anchor));
    }

    function moveGhost(x, y) {
        drag.ghost.style.transform = `translate(${x - drag.offsetX}px, ${y - drag.offsetY}px) scale(1.015)`;
    }

    // Near the top or the bottom of the window, the page scrolls itself.
    function autoScroll() {
        if (!drag || !scrollSpeed) {
            frame = 0;
            return;
        }
        window.scrollBy(0, scrollSpeed);
        placeAt(drag.x, drag.y);
        frame = window.requestAnimationFrame(autoScroll);
    }

    function startDrag(cell, event) {
        const box = cell.getBoundingClientRect();
        const ghost = cell.cloneNode(true);
        ghost.classList.add("dash-ghost");
        ghost.removeAttribute("data-module-id");
        ghost.setAttribute("aria-hidden", "true");
        ghost.style.width = `${box.width}px`;
        ghost.style.height = `${box.height}px`;
        document.body.append(ghost);
        drag = {
            cell,
            ghost,
            pointerId: event.pointerId,
            startOrder: orderKey(),
            home: cell.nextElementSibling,
            offsetX: pending.x - box.left,
            offsetY: pending.y - box.top,
            x: event.clientX,
            y: event.clientY,
        };
        cell.classList.add("is-placeholder");
        document.body.classList.add("is-dragging-block");
        moveGhost(event.clientX, event.clientY);
    }

    live.addEventListener("pointerdown", (event) => {
        const cell = arranging() ? event.target.closest(".dash-cell[data-module-id]") : null;
        if (!cell || event.target.closest("[data-size], [data-hide], [data-options], [data-options-toggle]")) return;
        if (drag || pending || (event.pointerType === "mouse" && event.button !== 0)) return;
        event.preventDefault();
        cell.setPointerCapture(event.pointerId);
        pending = { cell, pointerId: event.pointerId, x: event.clientX, y: event.clientY };
    });

    live.addEventListener("pointermove", (event) => {
        if (pending && !drag && event.pointerId === pending.pointerId) {
            const moved = Math.hypot(event.clientX - pending.x, event.clientY - pending.y);
            if (moved >= DRAG_THRESHOLD) startDrag(pending.cell, event);
        }
        if (!drag || event.pointerId !== drag.pointerId) return;
        drag.x = event.clientX;
        drag.y = event.clientY;
        moveGhost(event.clientX, event.clientY);
        placeAt(event.clientX, event.clientY);
        const top = event.clientY < EDGE;
        const bottom = event.clientY > window.innerHeight - EDGE;
        scrollSpeed = top ? -12 : bottom ? 12 : 0;
        if (scrollSpeed && !frame) frame = window.requestAnimationFrame(autoScroll);
    });

    // The lifted block settles into its slot, then the slot is the block.
    function land(cell, ghost) {
        const done = () => {
            ghost.remove();
            cell.classList.remove("is-placeholder");
        };
        if (reduced) {
            done();
            return;
        }
        const slot = cell.getBoundingClientRect();
        ghost
            .animate([{ transform: ghost.style.transform }, { transform: `translate(${slot.left}px, ${slot.top}px)` }], {
                duration: 220,
                easing: "cubic-bezier(0.16, 1, 0.3, 1)",
                fill: "forwards",
            })
            .finished.then(done, done);
    }

    function endDrag(event, cancel) {
        if (pending && !drag) {
            pending = null;
            return;
        }
        if (!drag || (event && event.pointerId !== drag.pointerId)) return;
        const { cell, ghost, startOrder, home } = drag;
        drag = null;
        pending = null;
        scrollSpeed = 0;
        document.body.classList.remove("is-dragging-block");
        if (cancel) flip(() => cell.parentElement.insertBefore(cell, home));
        land(cell, ghost);
        if (orderKey() === startOrder) return;
        announceMove(cell);
        saveOrder();
    }

    live.addEventListener("pointerup", (event) => endDrag(event, false));
    live.addEventListener("pointercancel", (event) => endDrag(event, true));
    document.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && drag) endDrag(null, true);
    });

    live.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && openOptions) {
            const cell = live.querySelector(`.dash-cell[data-module-id="${openOptions.moduleId}"]`);
            closeOptions();
            const button = cell && cell.querySelector("[data-options-toggle]");
            if (button) button.focus();
            return;
        }
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
        const hide = event.target.closest("[data-hide]");
        if (hide) {
            setShown(hide.closest(".dash-cell[data-module-id]").dataset.moduleId, false);
            return;
        }
        const optionsToggle = event.target.closest("[data-options-toggle]");
        if (optionsToggle) {
            const cell = optionsToggle.closest(".dash-cell[data-module-id]");
            const open = optionsToggle.getAttribute("aria-expanded") !== "true";
            closeOptions();
            setOptionsOpen(cell, open);
            return;
        }
        const size = event.target.closest("[data-size]");
        if (size) {
            setWidth(size.closest(".dash-cell[data-module-id]"), size.dataset.size);
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

// Reordering for the dashboard layout editor. A row is dragged by its handle
// with any pointer (mouse, pen or finger), or moved with the up and down arrow
// keys while its handle has focus. The order is saved in the background, so a
// drop or a key press never reloads the page and never loses the focus.
(function () {
    "use strict";

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    // Several quick key presses make one save, not one per press.
    const KEY_SAVE_DELAY = 600;

    function translateY(row) {
        const transform = getComputedStyle(row).transform;
        return transform && transform !== "none" ? new DOMMatrixReadOnly(transform).m42 : 0;
    }

    // Where the row sits in the layout, whatever glide or drag offset it has.
    function layoutTop(row) {
        return row.getBoundingClientRect().top - translateY(row);
    }

    // Puts a row back where it was drawn, then lets the stylesheet
    // transition carry it to its new place.
    function glide(row, delta) {
        if (Math.abs(delta) < 1) return;
        row.style.transition = "none";
        row.style.transform = `translateY(${delta}px)`;
        void row.offsetHeight;
        row.style.transition = "";
        row.style.transform = "";
    }

    class LayoutList {
        constructor(list, form) {
            this.list = list;
            this.form = form;
            this.status = document.getElementById("layout-status");
            this.saved = document.getElementById("layout-saved");
            this.drag = null;
            this.saveTimer = null;
            list.addEventListener("pointerdown", (event) => this.startDrag(event));
            list.addEventListener("pointermove", (event) => this.moveDrag(event));
            list.addEventListener("pointerup", (event) => this.endDrag(event));
            list.addEventListener("pointercancel", (event) => this.endDrag(event));
            list.addEventListener("keydown", (event) => this.moveWithKey(event));
            window.addEventListener("pagehide", () => {
                if (this.saveTimer) this.save();
            });
        }

        rows() {
            return Array.from(this.list.querySelectorAll("[data-module-row]"));
        }

        orderKey() {
            return this.rows().map((row) => row.dataset.moduleId || "").join(",");
        }

        // The plain submission reloads the page on the saved order: the way
        // back to a truthful screen when the background save is refused.
        submitOrder() {
            this.form.querySelectorAll('input[name="order"]').forEach((input) => input.remove());
            this.rows().forEach((row) => {
                const input = document.createElement("input");
                input.type = "hidden";
                input.name = "order";
                input.value = row.dataset.moduleId || "";
                this.form.appendChild(input);
            });
            this.form.submit();
        }

        save() {
            clearTimeout(this.saveTimer);
            this.saveTimer = null;
            const body = new URLSearchParams(new FormData(this.form));
            this.rows().forEach((row) => body.append("order", row.dataset.moduleId || ""));
            fetch(this.form.action, { method: "POST", body, credentials: "same-origin", keepalive: true })
                .then((response) => {
                    if (!response.ok) throw new Error(`order not saved: ${response.status}`);
                    this.confirmSaved();
                })
                .catch(() => this.submitOrder());
        }

        confirmSaved() {
            if (!this.saved) return;
            this.saved.hidden = false;
            if (this.status) this.status.textContent = this.saved.textContent.trim();
        }

        announce(row) {
            if (!this.status) return;
            const all = this.rows();
            this.status.textContent = this.status.dataset.template
                .replace("{name}", row.dataset.moduleName || "")
                .replace("{index}", String(all.indexOf(row) + 1))
                .replace("{total}", String(all.length));
        }

        // Neighbours glide to their new place instead of jumping; the row
        // being dragged follows the pointer instead.
        reorder(mutate) {
            if (reducedMotion.matches) {
                mutate();
                return;
            }
            const before = new Map(this.rows().map((row) => [row, row.getBoundingClientRect().top]));
            mutate();
            this.rows().forEach((row) => {
                if (this.drag && row === this.drag.row) return;
                glide(row, before.get(row) - layoutTop(row));
            });
        }

        // The dragged row belongs before the first other row whose halfway
        // line is below the pointer. Halfway lines come from the layout, not
        // from rows still gliding, so a pointer held still does not make the
        // order flip back and forth.
        anchorAt(y) {
            const dragged = this.drag.row;
            return this.rows().find((row) => row !== dragged && y < layoutTop(row) + row.offsetHeight / 2) || null;
        }

        // The row stays under the pointer, held inside the list.
        follow(y) {
            const { row, grabOffset } = this.drag;
            const bounds = this.list.getBoundingClientRect();
            const top = Math.min(Math.max(y - grabOffset, bounds.top), bounds.bottom - row.offsetHeight);
            row.style.transform = `translateY(${top - layoutTop(row)}px)`;
        }

        startDrag(event) {
            const handle = event.target.closest("[data-drag-handle]");
            if (!handle || this.drag || (event.pointerType === "mouse" && event.button !== 0)) return;
            event.preventDefault();
            const row = handle.closest("[data-module-row]");
            handle.setPointerCapture(event.pointerId);
            this.drag = {
                row,
                pointerId: event.pointerId,
                grabOffset: event.clientY - layoutTop(row),
                startOrder: this.orderKey(),
            };
            row.style.transition = "none";
            row.classList.add("is-dragging");
        }

        moveDrag(event) {
            if (!this.drag || event.pointerId !== this.drag.pointerId) return;
            const row = this.drag.row;
            const anchor = this.anchorAt(event.clientY);
            const unchanged = anchor === row.nextElementSibling || (anchor === null && row === this.list.lastElementChild);
            if (!unchanged) this.reorder(() => this.list.insertBefore(row, anchor));
            this.follow(event.clientY);
        }

        // Released onto the stylesheet transition, the row settles in its slot.
        endDrag(event) {
            if (!this.drag || event.pointerId !== this.drag.pointerId) return;
            const { row, startOrder } = this.drag;
            this.drag = null;
            row.classList.remove("is-dragging");
            row.style.transition = "";
            row.style.transform = "";
            if (this.orderKey() === startOrder) return;
            this.announce(row);
            this.save();
        }

        moveWithKey(event) {
            const handle = event.target.closest("[data-drag-handle]");
            const up = event.key === "ArrowUp";
            if (!handle || this.drag || (!up && event.key !== "ArrowDown")) return;
            event.preventDefault();
            const row = handle.closest("[data-module-row]");
            const sibling = up ? row.previousElementSibling : row.nextElementSibling;
            if (!sibling) return;
            this.reorder(() => this.list.insertBefore(row, up ? sibling : sibling.nextElementSibling));
            handle.focus();
            this.announce(row);
            clearTimeout(this.saveTimer);
            this.saveTimer = setTimeout(() => this.save(), KEY_SAVE_DELAY);
        }
    }

    document.addEventListener("DOMContentLoaded", () => {
        const form = document.getElementById("layout-order-form");
        if (!form) return;
        document.querySelectorAll("[data-modules-list]").forEach((list) => new LayoutList(list, form));
    });
})();

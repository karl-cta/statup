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

    function initList(listEl) {
        const form = document.getElementById("layout-order-form");
        if (!form) return;
        const status = document.getElementById("layout-status");
        const saved = document.getElementById("layout-saved");

        let drag = null;
        let saveTimer = null;

        function rows() {
            return Array.prototype.slice.call(
                listEl.querySelectorAll("[data-module-row]")
            );
        }

        function orderKey() {
            return rows().map(function (row) { return row.dataset.moduleId || ""; }).join(",");
        }

        // The plain submission reloads the page on the saved order: the way
        // back to a truthful screen when the background save is refused.
        function submitOrder() {
            form.querySelectorAll('input[name="order"]').forEach(function (i) { i.remove(); });
            rows().forEach(function (row) {
                const input = document.createElement("input");
                input.type = "hidden";
                input.name = "order";
                input.value = row.dataset.moduleId || "";
                form.appendChild(input);
            });
            form.submit();
        }

        function save() {
            clearTimeout(saveTimer);
            saveTimer = null;
            const body = new URLSearchParams(new FormData(form));
            rows().forEach(function (row) { body.append("order", row.dataset.moduleId || ""); });
            fetch(form.action, { method: "POST", body: body, credentials: "same-origin", keepalive: true })
                .then(function (response) {
                    if (!response.ok) throw new Error("order not saved: " + response.status);
                    if (saved) saved.hidden = false;
                })
                .catch(submitOrder);
        }

        function announce(row) {
            if (!status) return;
            const all = rows();
            status.textContent = status.dataset.template
                .replace("{name}", row.dataset.moduleName || "")
                .replace("{index}", String(all.indexOf(row) + 1))
                .replace("{total}", String(all.length));
        }

        // Neighbours glide to their new place instead of jumping. Each row's
        // position on screen is read before the move, the row is put back
        // there with a transform, then released onto the stylesheet
        // transition. The row being dragged follows the pointer instead.
        function reorder(mutate) {
            if (reducedMotion.matches) {
                mutate();
                return;
            }
            const before = new Map(rows().map(function (row) {
                return [row, row.getBoundingClientRect().top];
            }));
            mutate();
            rows().forEach(function (row) {
                if (drag && row === drag.row) return;
                const delta = before.get(row) - layoutTop(row);
                if (Math.abs(delta) < 1) return;
                row.style.transition = "none";
                row.style.transform = "translateY(" + delta + "px)";
                void row.offsetHeight;
                row.style.transition = "";
                row.style.transform = "";
            });
        }

        // The dragged row belongs before the first other row whose halfway
        // line is below the pointer. Halfway lines come from the layout, not
        // from rows still gliding, so a pointer held still does not make the
        // order flip back and forth.
        function anchorAt(y) {
            return rows().find(function (row) {
                return row !== drag.row && y < layoutTop(row) + row.offsetHeight / 2;
            }) || null;
        }

        // The row stays under the pointer, held inside the list.
        function follow(y) {
            const list = listEl.getBoundingClientRect();
            const top = Math.min(
                Math.max(y - drag.grabOffset, list.top),
                list.bottom - drag.row.offsetHeight
            );
            drag.row.style.transform = "translateY(" + (top - layoutTop(drag.row)) + "px)";
        }

        listEl.addEventListener("pointerdown", function (event) {
            const handle = event.target.closest("[data-drag-handle]");
            if (!handle || drag || (event.pointerType === "mouse" && event.button !== 0)) return;
            event.preventDefault();
            const row = handle.closest("[data-module-row]");
            handle.setPointerCapture(event.pointerId);
            drag = {
                row: row,
                pointerId: event.pointerId,
                grabOffset: event.clientY - layoutTop(row),
                startOrder: orderKey()
            };
            row.style.transition = "none";
            row.classList.add("is-dragging");
        });

        listEl.addEventListener("pointermove", function (event) {
            if (!drag || event.pointerId !== drag.pointerId) return;
            const anchor = anchorAt(event.clientY);
            const unchanged = anchor === drag.row.nextElementSibling
                || (anchor === null && drag.row === listEl.lastElementChild);
            if (!unchanged) {
                reorder(function () { listEl.insertBefore(drag.row, anchor); });
            }
            follow(event.clientY);
        });

        function endDrag(event) {
            if (!drag || event.pointerId !== drag.pointerId) return;
            const row = drag.row;
            const changed = orderKey() !== drag.startOrder;
            drag = null;
            row.classList.remove("is-dragging");
            // Released onto the stylesheet transition, the row settles in its slot.
            row.style.transition = "";
            row.style.transform = "";
            if (changed) {
                announce(row);
                save();
            }
        }

        listEl.addEventListener("pointerup", endDrag);
        listEl.addEventListener("pointercancel", endDrag);

        listEl.addEventListener("keydown", function (event) {
            const handle = event.target.closest("[data-drag-handle]");
            const up = event.key === "ArrowUp";
            if (!handle || drag || (!up && event.key !== "ArrowDown")) return;
            event.preventDefault();
            const row = handle.closest("[data-module-row]");
            const sibling = up ? row.previousElementSibling : row.nextElementSibling;
            if (!sibling) return;
            reorder(function () {
                listEl.insertBefore(row, up ? sibling : sibling.nextElementSibling);
            });
            handle.focus();
            announce(row);
            clearTimeout(saveTimer);
            saveTimer = setTimeout(save, KEY_SAVE_DELAY);
        });

        window.addEventListener("pagehide", function () {
            if (saveTimer) save();
        });
    }

    document.addEventListener("DOMContentLoaded", function () {
        document.querySelectorAll("[data-modules-list]").forEach(initList);
    });
})();

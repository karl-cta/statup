// Reordering for the dashboard layout editor.
// Native HTML5 drag and drop on desktop, plus tap-friendly up/down buttons that
// also work for keyboard users. After every reorder, the current order is
// submitted through the hidden form so the server can persist it.

(function () {
    "use strict";

    function initList(listEl) {
        const form = document.getElementById("layout-order-form");
        if (!form) return;

        let dragged = null;

        function rows() {
            return Array.prototype.slice.call(
                listEl.querySelectorAll("[data-module-row]")
            );
        }

        function refreshMoveButtons() {
            const all = rows();
            all.forEach(function (row, idx) {
                const up = row.querySelector('[data-move="up"]');
                const down = row.querySelector('[data-move="down"]');
                if (up) up.disabled = idx === 0;
                if (down) down.disabled = idx === all.length - 1;
            });
        }

        function submitOrder() {
            const orderInputs = form.querySelectorAll('input[name="order"]');
            orderInputs.forEach(function (i) { i.remove(); });
            rows().forEach(function (row) {
                const input = document.createElement("input");
                input.type = "hidden";
                input.name = "order";
                input.value = row.dataset.moduleId || "";
                form.appendChild(input);
            });
            form.submit();
        }

        function move(row, direction) {
            if (direction === "up") {
                const prev = row.previousElementSibling;
                if (prev && prev.matches("[data-module-row]")) {
                    listEl.insertBefore(row, prev);
                    submitOrder();
                }
            } else if (direction === "down") {
                const next = row.nextElementSibling;
                if (next && next.matches("[data-module-row]")) {
                    listEl.insertBefore(next, row);
                    submitOrder();
                }
            }
        }

        rows().forEach(function (row) {
            row.addEventListener("dragstart", function (event) {
                dragged = row;
                row.classList.add("is-dragging");
                if (event.dataTransfer) {
                    event.dataTransfer.effectAllowed = "move";
                    event.dataTransfer.setData("text/plain", row.dataset.moduleId || "");
                }
            });

            row.addEventListener("dragend", function () {
                row.classList.remove("is-dragging");
                dragged = null;
                submitOrder();
            });

            row.addEventListener("dragover", function (event) {
                event.preventDefault();
                if (!dragged || dragged === row) return;
                const rect = row.getBoundingClientRect();
                const before = (event.clientY - rect.top) < rect.height / 2;
                if (before) {
                    listEl.insertBefore(dragged, row);
                } else {
                    listEl.insertBefore(dragged, row.nextSibling);
                }
            });

            const moveButtons = row.querySelectorAll("[data-move]");
            moveButtons.forEach(function (btn) {
                btn.addEventListener("click", function (event) {
                    event.preventDefault();
                    move(row, btn.dataset.move);
                });
            });
        });

        refreshMoveButtons();
    }

    document.addEventListener("DOMContentLoaded", function () {
        document.querySelectorAll("[data-modules-list]").forEach(initList);
    });
})();

// Drag-and-drop reordering for the dashboard layout editor.
// Uses native HTML5 DnD. After each drop, submits the current order through
// the hidden form so the server can persist it.

(function () {
    "use strict";

    function initList(listEl) {
        const form = document.getElementById("layout-order-form");
        if (!form) return;

        let dragged = null;

        listEl.querySelectorAll("[data-module-row]").forEach(function (row) {
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
        });

        function submitOrder() {
            const orderInputs = form.querySelectorAll('input[name="order"]');
            orderInputs.forEach(function (i) { i.remove(); });
            listEl.querySelectorAll("[data-module-row]").forEach(function (row) {
                const input = document.createElement("input");
                input.type = "hidden";
                input.name = "order";
                input.value = row.dataset.moduleId || "";
                form.appendChild(input);
            });
            form.submit();
        }
    }

    document.addEventListener("DOMContentLoaded", function () {
        document.querySelectorAll("[data-modules-list]").forEach(initList);
    });
})();

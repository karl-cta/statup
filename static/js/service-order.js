// Choosing the order of services on the Services page. The mode gives each
// row two arrows; a row moves at once, the focus stays on the arrow pressed,
// and the whole order is saved shortly after the last press. A refused save
// reloads the page, the only way back to a truthful list.
(function () {
    "use strict";

    const SAVE_DELAY = 600;
    const list = document.querySelector("[data-service-order]");
    const toggle = document.querySelector("[data-order-toggle]");
    if (!list || !toggle) return;

    const { announce, csrfToken } = window.statup;
    let saveTimer = null;

    const rows = () => Array.from(list.querySelectorAll("[data-service-id]"));
    const ordering = () => list.classList.contains("is-ordering");

    function save() {
        saveTimer = null;
        const fields = rows().map((row) => ["order", row.dataset.serviceId]);
        fetch("/services/order", {
            method: "POST",
            body: new URLSearchParams(fields),
            credentials: "same-origin",
            headers: { "X-CSRF-Token": csrfToken() },
            keepalive: true,
        })
            .then((response) => {
                if (!response.ok) throw new Error(`not saved: ${response.status}`);
                announce(list.dataset.saved);
            })
            .catch(() => window.location.reload());
    }

    function saveSoon() {
        window.clearTimeout(saveTimer);
        saveTimer = window.setTimeout(save, SAVE_DELAY);
    }

    function flush() {
        if (saveTimer === null) return;
        window.clearTimeout(saveTimer);
        save();
    }

    function syncEnds() {
        const all = rows();
        all.forEach((row, index) => {
            row.querySelector('[data-move="up"]').disabled = index === 0;
            row.querySelector('[data-move="down"]').disabled = index === all.length - 1;
        });
    }

    function announceMove(row) {
        const all = rows();
        announce(
            (list.dataset.moved || "")
                .replace("{name}", row.dataset.serviceName)
                .replace("{index}", String(all.indexOf(row) + 1))
                .replace("{total}", String(all.length)),
        );
    }

    function move(button) {
        const row = button.closest("[data-service-id]");
        const up = button.dataset.move === "up";
        const neighbour = up ? row.previousElementSibling : row.nextElementSibling;
        if (!neighbour) return;
        neighbour.insertAdjacentElement(up ? "beforebegin" : "afterend", row);
        syncEnds();
        // At the top or the bottom the pressed arrow is disabled; the other
        // one keeps the focus in the row.
        const other = row.querySelector(`[data-move="${up ? "down" : "up"}"]`);
        (button.disabled ? other : button).focus();
        announceMove(row);
        saveSoon();
    }

    function setMode(on) {
        list.classList.toggle("is-ordering", on);
        toggle.setAttribute("aria-pressed", String(on));
        if (!on) flush();
    }

    toggle.addEventListener("click", () => setMode(!ordering()));
    list.addEventListener("click", (event) => {
        const button = event.target.closest("[data-move]");
        if (button && ordering()) move(button);
    });
    document.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && ordering()) {
            setMode(false);
            toggle.focus();
        }
    });
    window.addEventListener("pagehide", flush);

    syncEnds();
    toggle.hidden = false;
})();

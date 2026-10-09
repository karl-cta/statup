// Choosing the order of services on the Services page. The mode gives each
// row two arrows; a row moves at once, the focus stays on the arrow pressed,
// and the whole order is saved shortly after the last press. A refused save
// reloads the page, the only way back to a truthful list. With services
// that have a problem listed first, the mode puts back the chosen order, and
// leaving it reloads the page to list them first again.
(function () {
    "use strict";

    const SAVE_DELAY = 600;
    const list = document.querySelector("[data-service-order]");
    const toggle = document.querySelector("[data-order-toggle]");
    if (!list || !toggle) return;
    const problemsFirst = document.querySelector("[data-problems-first]");
    const problemsFirstField = document.querySelector("[data-problems-first-field]");

    const { announce, csrfToken } = window.statup;
    let saveTimer = null;
    let sending = Promise.resolve();

    const rows = () => Array.from(list.querySelectorAll("[data-service-id]"));
    const ordering = () => list.classList.contains("is-ordering");

    function post(url, fields) {
        return fetch(url, {
            method: "POST",
            body: new URLSearchParams(fields),
            credentials: "same-origin",
            headers: { "X-CSRF-Token": csrfToken() },
            keepalive: true,
        }).then((response) => {
            if (!response.ok) throw new Error(`not saved: ${response.status}`);
            announce(list.dataset.saved);
        });
    }

    function send(fields) {
        return post("/services/order", fields);
    }

    // One save at a time, so an older order never lands after a newer one;
    // a page being left cannot wait for its turn.
    function save(leaving) {
        saveTimer = null;
        const fields = rows().map((row) => ["order", row.dataset.serviceId]);
        const next = leaving ? send(fields) : sending.then(() => send(fields));
        sending = next.catch(() => window.location.reload());
    }

    function saveSoon() {
        window.clearTimeout(saveTimer);
        saveTimer = window.setTimeout(save, SAVE_DELAY);
    }

    function flush(leaving) {
        if (saveTimer === null) return;
        window.clearTimeout(saveTimer);
        save(leaving);
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
                .replace("{name}", () => row.dataset.serviceName)
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

    function putBackChosenOrder() {
        rows()
            .sort((a, b) => Number(a.dataset.place) - Number(b.dataset.place))
            .forEach((row) => list.appendChild(row));
        syncEnds();
    }

    function setMode(on) {
        if (on && problemsFirst?.checked) putBackChosenOrder();
        list.classList.toggle("is-ordering", on);
        toggle.setAttribute("aria-pressed", String(on));
        if (problemsFirstField) problemsFirstField.hidden = !on;
        if (on) return;
        flush(false);
        if (problemsFirst?.checked) sending.then(() => window.location.reload());
    }

    problemsFirst?.addEventListener("change", () => {
        const fields = [["enabled", String(problemsFirst.checked)]];
        sending = sending
            .then(() => post("/services/problems-first", fields))
            .catch(() => window.location.reload());
    });

    toggle.addEventListener("click", () => setMode(!ordering()));
    list.addEventListener("click", (event) => {
        const button = event.target.closest("[data-move]");
        if (button && ordering()) move(button);
    });
    // Escape closes the mode from its own controls only, not from a menu
    // opened meanwhile.
    document.addEventListener("keydown", (event) => {
        const own = list.contains(document.activeElement) || document.activeElement === toggle;
        if (event.key !== "Escape" || event.defaultPrevented || !ordering() || !own) return;
        setMode(false);
        toggle.focus();
    });
    window.addEventListener("pagehide", () => flush(true));

    syncEnds();
    toggle.hidden = false;
})();

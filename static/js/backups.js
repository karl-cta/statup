// The schedule of the automatic backups: the day only counts for a weekly
// backup, and the time and the count not at all while they are off.
(function () {
    "use strict";

    for (const form of document.querySelectorAll("[data-backup-schedule]")) {
        const when = form.querySelector("[data-backup-when]");
        const weekly = form.querySelector("[data-weekly-only]");
        const redraw = () => {
            const chosen = form.querySelector('input[name="frequency"]:checked')?.value;
            when.hidden = chosen === "off";
            weekly.hidden = chosen !== "weekly";
        };
        form.addEventListener("change", redraw);
    }
})();

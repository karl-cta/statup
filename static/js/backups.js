// The schedule of the automatic backups: the day only counts for a weekly
// backup, and the time and the count not at all while they are off. The
// day's menu is as wide as its longest day, measured once the panel shows.
(function () {
    "use strict";

    function fitToLongestDay(select) {
        const wrap = select.closest(".cs-wrap");
        const label = wrap && wrap.querySelector(".cs-face .cs-label");
        if (!label || wrap.dataset.fitted) return;
        const shown = label.textContent;
        wrap.style.width = "max-content";
        let widest = 0;
        for (const option of select.options) {
            label.textContent = option.textContent;
            widest = Math.max(widest, wrap.getBoundingClientRect().width);
        }
        label.textContent = shown;
        if (widest === 0) return;
        wrap.style.width = `${Math.ceil(widest)}px`;
        wrap.dataset.fitted = "1";
    }

    for (const form of document.querySelectorAll("[data-backup-schedule]")) {
        const when = form.querySelector("[data-backup-when]");
        const weekly = form.querySelector("[data-weekly-only]");
        const day = form.querySelector('select[name="weekday"]');
        const redraw = () => {
            const chosen = form.querySelector('input[name="frequency"]:checked')?.value;
            when.hidden = chosen === "off";
            weekly.hidden = chosen !== "weekly";
            if (!weekly.hidden) fitToLongestDay(day);
        };
        form.addEventListener("change", redraw);
        form.closest("details")?.addEventListener("toggle", redraw);
    }
})();

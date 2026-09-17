// Keeps the uptime tooltip on the day it describes without letting it leave
// the card. Centering it in CSS is not an option here: the sparkline lives in
// a 260px column, so a bar is about 6px wide and the label sixteen times that.
// The CSS fallback pins the label to whichever end of the row is nearest,
// which never overflows but sits far from the hovered day.
(function () {
    "use strict";

    let probe = null;
    let widths = Object.create(null);

    function measure(bar, text) {
        const cached = widths[text];
        if (cached !== undefined) return cached;
        if (!probe) {
            probe = document.createElement("span");
            probe.setAttribute("aria-hidden", "true");
            probe.style.cssText = "position:absolute;left:-9999px;top:0;white-space:pre;pointer-events:none";
            document.body.appendChild(probe);
        }
        const style = window.getComputedStyle(bar, "::after");
        probe.style.fontFamily = style.fontFamily;
        probe.style.fontSize = style.fontSize;
        probe.style.fontWeight = style.fontWeight;
        probe.style.letterSpacing = style.letterSpacing;
        probe.style.padding = style.padding;
        probe.textContent = text;
        const width = probe.getBoundingClientRect().width;
        widths[text] = width;
        return width;
    }

    function place(bar, row) {
        const text = `${bar.getAttribute("data-day-date") || ""}\n${bar.getAttribute("data-day-status") || ""}`;
        const width = measure(bar, text);
        const center = bar.offsetLeft + bar.offsetWidth / 2;
        const rightmost = Math.max(row.clientWidth - width, 0);
        const left = Math.min(Math.max(center - width / 2, 0), rightmost);
        row.style.setProperty("--tt-x", `${Math.round(left)}px`);
        // Set on first use rather than at load, so rows swapped in by htmx are
        // covered without a second pass.
        row.classList.add("tip-js");
    }

    function onReveal(event) {
        // Deliberately not closest(): this runs for every element the pointer
        // crosses on the page, so the common case has to be two class checks.
        const bar = event.target;
        if (!bar || !bar.classList || !bar.classList.contains("bar")) return;
        const row = bar.parentElement;
        if (row && row.classList.contains("svc-bars")) place(bar, row);
    }

    document.addEventListener("pointerover", onReveal);
    // Cached widths are in pixels, so a zoom or a font swap invalidates them.
    window.addEventListener("resize", () => {
        widths = Object.create(null);
    });
})();

// A language switch reloads the page it was made on. Loaded before the
// first paint so that the page comes back where the reader was.
(function () {
    const KEY = "language-switch";
    const here = () => location.pathname + location.search;

    document.addEventListener("click", (event) => {
        const link = event.target instanceof Element ? event.target.closest('a[href^="/i18n?"]') : null;
        if (!link || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey) return;
        const menu = link.closest("details");
        if (menu) menu.open = false;
        try {
            sessionStorage.setItem(KEY, JSON.stringify({ page: here(), y: window.scrollY }));
        } catch {
            // Without storage the page simply reloads at its top.
        }
    });

    let saved = null;
    try {
        saved = JSON.parse(sessionStorage.getItem(KEY));
        sessionStorage.removeItem(KEY);
    } catch {
        saved = null;
    }
    if (!saved || saved.page !== here()) return;

    // At once: the page scrolls smoothly otherwise, and would slide down
    // from its top.
    const restore = () => window.scrollTo({ top: saved.y, behavior: "instant" });
    window.addEventListener("pagereveal", restore);
    document.addEventListener("DOMContentLoaded", restore);
})();

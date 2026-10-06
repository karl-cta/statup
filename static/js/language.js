// A language switch reloads the page it was made on. Loaded before the
// first paint so that the page comes back where the reader was, its words
// changing in place rather than sliding in like another page.
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

    const restore = () => window.scrollTo(0, saved.y);
    window.addEventListener("pagereveal", (event) => {
        if (event.viewTransition) event.viewTransition.types.add("language");
        restore();
    });
    document.addEventListener("DOMContentLoaded", restore);
})();

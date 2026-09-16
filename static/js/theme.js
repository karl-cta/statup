// Runs before the first paint so a dark page never flashes white.
(function () {
    const root = document.documentElement;
    root.classList.add("js");
    let stored = null;
    try {
        stored = window.localStorage.getItem("theme");
    } catch {
        stored = null;
    }
    const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    if (stored === "dark" || (stored === null && prefersDark)) {
        root.classList.add("dark");
    }
})();

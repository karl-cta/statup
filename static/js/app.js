// Behaviour shared by every page. Loaded deferred, after htmx on the pages
// that use it.
(function () {
    "use strict";

    const root = document.documentElement;
    const LIVE_REFRESH_MS = 60000;
    // A side panel that has not loaded by then offers to try again.
    const DRAWER_TIMEOUT_MS = 15000;
    const FOCUSABLE =
        'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), ' +
        'select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

    // htmx sends the session token with every request it makes.
    document.body.addEventListener("htmx:configRequest", (event) => {
        const meta = document.querySelector('meta[name="csrf-token"]');
        if (meta && meta.content) {
            event.detail.headers["X-CSRF-Token"] = meta.content;
        }
        // Empty filters stay out of the address the list pushes.
        if (event.detail.verb === "get") {
            const parameters = event.detail.parameters;
            Object.keys(parameters).forEach((name) => {
                if (parameters[name] === "") delete parameters[name];
            });
        }
    });

    // htmx leaves error responses unswapped; the server sends a short message
    // for them, shown in the toast instead of failing in silence. The
    // dashboard's own refresh reports under its banner instead.
    function liveTarget(event) {
        const target = event.detail.target;
        return target instanceof Element && target.hasAttribute("data-live") ? target : null;
    }

    document.body.addEventListener("htmx:beforeSwap", (event) => {
        const live = liveTarget(event);
        if (live) {
            onLiveResponse(live, event);
            return;
        }
        if (event.detail.xhr.status < 400) {
            clearToast();
            return;
        }
        event.detail.shouldSwap = false;
        showToast(event.detail.xhr.responseText);
    });

    document.body.addEventListener("htmx:sendError", (event) => {
        const live = liveTarget(event);
        if (live) {
            markStale(live, true);
            return;
        }
        const toast = document.getElementById("toast");
        if (toast) showToastText(toast.dataset.offline || "");
    });

    function showToast(html) {
        const toast = document.getElementById("toast");
        if (!toast) return;
        const holder = document.createElement("template");
        holder.innerHTML = html;
        const message = holder.content.querySelector(".form-error");
        if (!message) {
            showToastText(toast.dataset.failed || "");
            return;
        }
        // The toast is the live region; the page may hold the same id.
        message.removeAttribute("role");
        message.removeAttribute("id");
        putToast(toast, message);
    }

    function showToastText(text) {
        const toast = document.getElementById("toast");
        if (!toast || !text) return;
        const box = document.createElement("div");
        box.className = "form-error";
        const line = document.createElement("p");
        line.textContent = text;
        box.append(line);
        putToast(toast, box);
    }

    function putToast(toast, box) {
        const close = document.getElementById("toast-close");
        if (close) box.append(close.content.cloneNode(true));
        toast.replaceChildren(box);
    }

    function clearToast() {
        const toast = document.getElementById("toast");
        if (toast) toast.replaceChildren();
    }

    // Messages that appear together with their text are not read by every
    // screen reader; this region always exists, so writing to it is.
    function announce(text) {
        const region = document.getElementById("announcer");
        if (!region || !text) return;
        region.textContent = "";
        window.setTimeout(() => {
            region.textContent = text;
        }, 100);
    }

    // Theme.
    function syncThemeButtons() {
        const dark = root.classList.contains("dark");
        document.querySelectorAll("[data-theme-toggle]").forEach((button) => {
            button.setAttribute("aria-pressed", String(dark));
        });
    }

    function toggleTheme() {
        const dark = root.classList.toggle("dark");
        try {
            window.localStorage.setItem("theme", dark ? "dark" : "light");
        } catch {
            // The choice then lasts for this page only.
        }
        syncThemeButtons();
    }

    // Compact menu.
    function setMenu(open) {
        const button = document.querySelector("[data-menu-toggle]");
        const menu = document.getElementById("mast-menu");
        if (!button || !menu) return;
        menu.hidden = !open;
        button.setAttribute("aria-expanded", String(open));
    }

    function menuIsOpen() {
        const menu = document.getElementById("mast-menu");
        return Boolean(menu && !menu.hidden);
    }

    // The tab of the section; "page" only on the section's own address.
    function markCurrentSection() {
        const shell = document.querySelector("[data-section]");
        const section = shell ? shell.dataset.section : "";
        if (!section) return;
        document.querySelectorAll(`[data-nav="${section}"]`).forEach((link) => {
            const here = link.getAttribute("href") === window.location.pathname;
            link.setAttribute("aria-current", here ? "page" : "true");
        });
    }

    // The instance zone, as the server gave it with the page.
    function clockOffset() {
        const zone = document.querySelector("[data-clock-offset]");
        return zone ? Number(zone.dataset.clockOffset) || 0 : 0;
    }

    function instanceDay() {
        return Math.floor((Date.now() + clockOffset() * 60000) / 86400000);
    }

    // Side panel with an event's detail.
    const drawer = { request: 0, url: "", opener: null, controller: null };

    function drawerParts() {
        return {
            panel: document.getElementById("drawer"),
            overlay: document.getElementById("drawer-overlay"),
            content: document.getElementById("drawer-content"),
        };
    }

    function drawerIsOpen() {
        const { panel } = drawerParts();
        return Boolean(panel && panel.classList.contains("is-open"));
    }

    function drawerMessage(panel, kind) {
        const box = document.createElement("div");
        if (kind === "loading") {
            box.className = "drawer-loader";
            box.setAttribute("role", "status");
            const spinner = document.createElement("div");
            spinner.className = "spinner";
            const text = document.createElement("span");
            text.className = "sr-only";
            text.textContent = panel.dataset.loading || "";
            box.append(spinner, text);
            return box;
        }
        box.className = "drawer-error";
        const text = document.createElement("p");
        text.textContent = panel.dataset.error || "";
        const retry = document.createElement("button");
        retry.type = "button";
        retry.className = "btn btn-ghost btn-sm";
        retry.dataset.drawerRetry = "";
        retry.textContent = panel.dataset.retry || "";
        box.append(text, retry);
        return box;
    }

    function stopDrawerRequest() {
        drawer.request++;
        if (drawer.controller) drawer.controller.abort();
        drawer.controller = null;
    }

    function fillDrawer(panel, content, html) {
        content.innerHTML = html;
        const title = content.querySelector("#drawer-title");
        if (!title) return;
        title.setAttribute("tabindex", "-1");
        panel.setAttribute("aria-labelledby", title.id);
    }

    function openDrawer(url, opener) {
        const { panel, overlay, content } = drawerParts();
        if (!panel || !overlay || !content) return false;
        stopDrawerRequest();
        const request = drawer.request;
        const controller = new AbortController();
        drawer.controller = controller;
        drawer.url = url;
        if (opener && !panel.contains(opener)) drawer.opener = opener;
        content.replaceChildren(drawerMessage(panel, "loading"));
        panel.removeAttribute("aria-labelledby");
        panel.removeAttribute("inert");
        overlay.classList.add("is-open");
        panel.classList.add("is-open");
        panel.focus({ preventScroll: true });
        const timer = window.setTimeout(() => controller.abort(), DRAWER_TIMEOUT_MS);
        fetch(url, { headers: { "HX-Request": "true" }, credentials: "same-origin", signal: controller.signal })
            .then((response) => {
                // A page for members only, reached once the session ended.
                const away = response.headers.get("HX-Redirect");
                if (away) {
                    window.location.assign(away);
                    return null;
                }
                if (!response.ok) throw new Error(String(response.status));
                return response.text();
            })
            .then((html) => {
                if (html !== null && request === drawer.request) fillDrawer(panel, content, html);
            })
            .catch(() => {
                if (request === drawer.request) {
                    content.replaceChildren(drawerMessage(panel, "error"));
                }
            })
            .finally(() => window.clearTimeout(timer));
        return true;
    }

    function closeDrawer() {
        const { panel, overlay } = drawerParts();
        if (!panel || !overlay || !drawerIsOpen()) return;
        stopDrawerRequest();
        panel.classList.remove("is-open");
        overlay.classList.remove("is-open");
        panel.setAttribute("inert", "");
        if (drawer.opener && document.contains(drawer.opener)) {
            drawer.opener.focus({ preventScroll: true });
        }
        drawer.opener = null;
    }

    function trapDrawerFocus(event) {
        const { panel } = drawerParts();
        const items = Array.from(panel.querySelectorAll(FOCUSABLE));
        if (items.length === 0) {
            event.preventDefault();
            panel.focus();
            return;
        }
        const first = items[0];
        const last = items[items.length - 1];
        if (event.shiftKey && (document.activeElement === first || document.activeElement === panel)) {
            event.preventDefault();
            last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
        }
    }

    // A question asked in place before a destructive post.
    function openConfirm(opener) {
        const form = opener.closest("[data-confirm]");
        const box = form && form.querySelector("[data-confirm-box]");
        if (!box) return;
        opener.hidden = true;
        box.hidden = false;
        const cancel = box.querySelector("[data-confirm-cancel]");
        if (cancel) cancel.focus();
    }

    function closeConfirm(inside) {
        const form = inside.closest("[data-confirm]");
        const box = inside.closest("[data-confirm-box]");
        if (!form || !box) return;
        box.hidden = true;
        const opener = form.querySelector("[data-confirm-open]");
        if (opener) {
            opener.hidden = false;
            opener.focus();
        }
    }

    // Escape answers "Cancel" to any question asked in place.
    function cancelQuestion(target) {
        const box = target.closest("[data-confirm-box], [data-status-confirm], [data-role-confirm]");
        const cancel = box && box.querySelector("[data-confirm-cancel], [data-status-cancel], [data-role-cancel]");
        if (cancel) cancel.click();
    }

    function copyValue(button) {
        const field = document.querySelector(button.dataset.copy);
        if (!field) return;
        const label = button.dataset.label || button.textContent;
        button.dataset.label = label;
        const done = () => {
            button.textContent = button.dataset.copied || label;
            announce(button.dataset.copied || "");
            window.setTimeout(() => {
                button.textContent = label;
            }, 2000);
        };
        // A page served over plain http has no clipboard interface; the
        // older command still copies there.
        const fallback = () => {
            field.select();
            if (document.execCommand("copy")) done();
        };
        if (navigator.clipboard && window.isSecureContext) {
            navigator.clipboard.writeText(field.value).then(done, fallback);
        } else {
            fallback();
        }
    }

    // The compact menu closes on any click outside it.
    function onMenuClick(target) {
        if (target.closest("[data-menu-toggle]")) {
            setMenu(!menuIsOpen());
            return true;
        }
        if (menuIsOpen() && !target.closest("#mast-menu")) setMenu(false);
        return false;
    }

    function onDrawerClick(target, event) {
        const link = target.closest("[data-drawer]");
        if (link && !event.metaKey && !event.ctrlKey && !event.shiftKey && event.button === 0) {
            if (openDrawer(link.dataset.drawer, link)) event.preventDefault();
            return true;
        }
        if (target.closest("[data-drawer-close]")) {
            closeDrawer();
            return true;
        }
        if (target.closest("[data-drawer-retry]") && drawer.url) {
            openDrawer(drawer.url, null);
            return true;
        }
        return false;
    }

    function onConfirmClick(target) {
        const opener = target.closest("[data-confirm-open]");
        if (opener) {
            openConfirm(opener);
            return true;
        }
        const cancel = target.closest("[data-confirm-cancel]");
        if (cancel) {
            closeConfirm(cancel);
            return true;
        }
        return false;
    }

    document.addEventListener("click", (event) => {
        const target = event.target;
        if (!(target instanceof Element)) return;
        if (target.closest("[data-theme-toggle]")) {
            toggleTheme();
            return;
        }
        if (onMenuClick(target)) return;
        if (target.closest("[data-toast-close]")) {
            clearToast();
            return;
        }
        if (onDrawerClick(target, event) || onConfirmClick(target)) return;
        const copy = target.closest("[data-copy]");
        if (copy) copyValue(copy);
    });

    document.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
            if (drawerIsOpen()) {
                closeDrawer();
                return;
            }
            if (menuIsOpen()) {
                setMenu(false);
                const button = document.querySelector("[data-menu-toggle]");
                if (button) button.focus();
                return;
            }
            if (event.target instanceof Element) cancelQuestion(event.target);
            return;
        }
        if (event.key === "Tab" && drawerIsOpen()) trapDrawerFocus(event);

        const form = event.target instanceof Element ? event.target.closest("form[data-autosubmit]") : null;
        if (form) delete form.dataset.pointer;
    });

    // A choice that applies itself under the pointer; from the keyboard the
    // arrows only move the choice and a button applies it.
    document.addEventListener("pointerdown", (event) => {
        const form = event.target instanceof Element ? event.target.closest("form[data-autosubmit]") : null;
        if (form) form.dataset.pointer = "1";
    });

    document.addEventListener("change", (event) => {
        const target = event.target;
        if (!(target instanceof Element)) return;
        const auto = target.closest("form[data-autosubmit]");
        if (auto) {
            const apply = auto.querySelector("[data-apply]");
            if (auto.dataset.pointer === "1") {
                auto.requestSubmit();
            } else if (apply) {
                apply.hidden = false;
            }
            return;
        }
        const upload = target.closest("form[data-submit-on-change]");
        if (upload && target.matches('input[type="file"]') && target.files.length > 0) {
            upload.requestSubmit();
        }
    });

    document.addEventListener("focusin", (event) => {
        const field = event.target;
        if (field instanceof HTMLInputElement && field.hasAttribute("data-select-on-focus")) {
            field.select();
        }
    });

    // An uploaded icon whose file is gone disappears instead of showing a
    // broken image.
    document.addEventListener(
        "error",
        (event) => {
            const image = event.target;
            if (image instanceof HTMLImageElement && image.hasAttribute("data-hide-on-error")) {
                image.remove();
            }
        },
        true,
    );

    function hideBrokenImages(scope) {
        scope.querySelectorAll("img[data-hide-on-error]").forEach((image) => {
            if (image.complete && image.naturalWidth === 0) image.remove();
        });
    }

    // The dashboard refreshes itself while the tab is visible, except while
    // the reader is inside it or in the side panel. A new day reloads the
    // page, whose date the server writes.
    function startLiveRefresh() {
        const live = document.querySelector("[data-live]");
        if (!live || !window.htmx) return;
        const day = instanceDay();
        let last = Date.now();
        const refresh = () => {
            if (document.visibilityState !== "visible" || drawerIsOpen() || live.contains(document.activeElement)) return;
            if (document.body.classList.contains("is-arranging")) return;
            if (instanceDay() !== day) {
                window.location.reload();
                return;
            }
            last = Date.now();
            window.htmx.ajax("GET", live.dataset.live, { target: live, swap: "innerHTML" }).catch(() => {});
        };
        window.setInterval(refresh, LIVE_REFRESH_MS);
        document.addEventListener("visibilitychange", () => {
            if (Date.now() - last > LIVE_REFRESH_MS) refresh();
        });
    }

    // The page is replaced only when what it shows has changed; otherwise
    // only its time stamp moves. A moved instance clock (daylight saving)
    // reloads it.
    function onLiveResponse(live, event) {
        const xhr = event.detail.xhr;
        if (xhr.status >= 400) {
            event.detail.shouldSwap = false;
            markStale(live, true);
            return;
        }
        markStale(live, false);
        const offset = xhr.getResponseHeader("X-Clock-Offset");
        if (offset !== null && Number(offset) !== clockOffset()) {
            event.detail.shouldSwap = false;
            window.location.reload();
            return;
        }
        const version = xhr.getResponseHeader("X-Live-Version");
        if (version && version === live.dataset.version) {
            event.detail.shouldSwap = false;
            copyStamp(live, xhr.responseText);
        } else if (version) {
            live.dataset.version = version;
        }
    }

    function copyStamp(live, html) {
        const holder = document.createElement("template");
        holder.innerHTML = html;
        const fresh = holder.content.querySelector("[data-stamp]");
        const current = live.querySelector("[data-stamp]");
        if (fresh && current) current.textContent = fresh.textContent;
    }

    // Two refreshes in a row without an answer are said under the banner.
    function markStale(live, failed) {
        const failures = failed ? Number(live.dataset.failures || 0) + 1 : 0;
        live.dataset.failures = String(failures);
        const note = live.querySelector("[data-stale]");
        if (note) note.hidden = failures < 2;
    }

    document.body.addEventListener("htmx:afterSettle", (event) => {
        const settled = event.target;
        if (!(settled instanceof Element)) return;
        const message = settled.matches("[data-announce]") ? settled : settled.querySelector("[data-announce]");
        if (message) announce(message.textContent.replace(/\s+/g, " ").trim());
        // The events list announces its new count after a filter change.
        if (settled.id !== "events-results") return;
        const form = document.querySelector("[data-results-live]");
        const region = form ? document.querySelector(form.dataset.resultsLive) : null;
        const count = settled.querySelector("[data-results-count]");
        if (region) region.textContent = count ? count.textContent.trim() : "";
    });

    function onReady() {
        markCurrentSection();
        syncThemeButtons();
        startLiveRefresh();
        hideBrokenImages(document);
        document.querySelectorAll("form[data-autosubmit] [data-apply]").forEach((button) => {
            button.hidden = true;
        });
        const saved = document.querySelector("[data-scroll-into-view]");
        if (saved) saved.scrollIntoView({ block: "center" });
        window.requestAnimationFrame(() => {
            window.setTimeout(() => root.classList.add("is-settled"), 800);
        });
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", onReady);
    } else {
        onReady();
    }
})();

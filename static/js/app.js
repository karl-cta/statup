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

    // The first account tells the instance which zone its team lives in.
    document.querySelectorAll("[data-browser-zone]").forEach((field) => {
        field.value = Intl.DateTimeFormat().resolvedOptions().timeZone || "";
    });

    // The session token the page was drawn with, empty on a page that
    // posts nothing.
    function csrfToken() {
        const meta = document.querySelector('meta[name="csrf-token"]');
        return meta ? meta.content : "";
    }

    // htmx sends the session token with every request it makes.
    document.body.addEventListener("htmx:configRequest", (event) => {
        const token = csrfToken();
        if (token) event.detail.headers["X-CSRF-Token"] = token;
        // Empty filters stay out of the address the list pushes.
        if (event.detail.verb === "get") {
            const parameters = event.detail.parameters;
            Object.keys(parameters).forEach((name) => {
                if (parameters[name] === "") delete parameters[name];
            });
        }
    });

    // Something that just arrived or changed lights up once.
    function flash(element) {
        element.classList.remove("is-fresh");
        void element.offsetWidth;
        element.classList.add("is-fresh");
        element.addEventListener("animationend", () => element.classList.remove("is-fresh"), { once: true });
    }

    // What the refreshed dashboard shows that it did not a moment ago: a
    // banner that changed, a service newly disrupted, a new event.
    const liveBefore = new WeakMap();

    function rowKey(row) {
        const link = row.matches("a") ? row : row.querySelector("a");
        return `${link ? link.getAttribute("href") : ""}|${row.dataset.tone || ""}`;
    }

    function liveSnapshot(live) {
        const headline = live.querySelector("#status-headline");
        return {
            headline: headline ? headline.textContent.replace(/\s+/g, " ").trim() : "",
            rows: new Set(Array.from(live.querySelectorAll(".hero-item, .log-row"), rowKey)),
        };
    }

    function lightChanges(live) {
        const before = liveBefore.get(live);
        if (!before) return;
        liveBefore.delete(live);
        const now = liveSnapshot(live);
        const hero = live.querySelector(".hero");
        if (hero && now.headline !== before.headline) flash(hero);
        live.querySelectorAll(".hero-item, .log-row").forEach((row) => {
            if (!before.rows.has(rowKey(row))) flash(row);
        });
    }

    function liveTarget(event) {
        const target = event.detail.target;
        return target instanceof Element && target.hasAttribute("data-live") ? target : null;
    }

    // htmx leaves error responses unswapped; the server sends a short message
    // for them, shown in the toast instead of failing in silence. The
    // dashboard's own refresh reports under its banner instead.
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

    // An update posted from the side panel: the page behind it is stale.
    document.body.addEventListener("event-updated", () => {
        drawer.changed = true;
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

    // For the page scripts, loaded after this one.
    window.statup = { announce, csrfToken };

    // Theme.
    function syncThemeButtons() {
        const dark = root.classList.contains("dark");
        document.querySelectorAll("[data-theme-toggle]").forEach((button) => {
            button.setAttribute("aria-pressed", String(dark));
        });
        document.querySelectorAll("[data-theme-choice]").forEach((button) => {
            button.setAttribute("aria-pressed", String((button.dataset.themeChoice === "dark") === dark));
        });
    }

    function setTheme(dark) {
        root.classList.toggle("dark", dark);
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

    // The masthead clock reads the instance's time, in the page's language.
    function tickClock() {
        const shown = document.querySelectorAll("[data-clock-time]");
        if (!shown.length) return;
        const at = new Date(Date.now() + clockOffset() * 60000);
        const minutes = String(at.getUTCMinutes()).padStart(2, "0");
        const hours = at.getUTCHours();
        const text = root.lang.startsWith("fr")
            ? `${String(hours).padStart(2, "0")}:${minutes}`
            : `${hours % 12 || 12}:${minutes} ${hours < 12 ? "AM" : "PM"}`;
        shown.forEach((element) => {
            if (element.textContent !== text) element.textContent = text;
        });
    }

    function instanceDay() {
        return Math.floor((Date.now() + clockOffset() * 60000) / 86400000);
    }

    // Side panel with an event's detail.
    // `changed` records an update posted from the panel, so the page behind
    // it is drawn again once the panel closes.
    const drawer = { request: 0, url: "", opener: null, controller: null, changed: false };

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
        retry.className = "btn btn-ghost";
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
        // The panel's own forms post in place, and its menus are dressed
        // like any content htmx brings in.
        if (window.htmx) {
            window.htmx.process(content);
            window.htmx.trigger(content, "htmx:load", { elt: content });
        }
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
        if (drawer.changed) redrawAfterPanel();
    }

    function redrawAfterPanel() {
        drawer.changed = false;
        const live = document.querySelector("[data-live]");
        if (live && window.htmx) {
            window.htmx.ajax("GET", live.dataset.live, { target: live, swap: "innerHTML" }).catch(() => {});
        } else {
            window.location.reload();
        }
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
        // A field holds its text as a value, a code element as its content.
        const text = "value" in field ? field.value : field.textContent.trim();
        // A page served over plain http has no clipboard interface; the
        // older command still copies there.
        const fallback = () => {
            if ("select" in field) field.select();
            else window.getSelection().selectAllChildren(field);
            if (document.execCommand("copy")) done();
        };
        if (navigator.clipboard && window.isSecureContext) {
            navigator.clipboard.writeText(text).then(done, fallback);
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

    // The folded actions of an event close on any click outside them.
    function closeMoreActions(except) {
        document.querySelectorAll("details[data-more-actions][open]").forEach((menu) => {
            if (menu !== except) menu.open = false;
        });
    }

    // A composer folded behind a button, as in the side panel.
    function onComposerToggle(target) {
        const button = target.closest("[data-composer-toggle]");
        if (!button) return false;
        const composer = document.getElementById(button.getAttribute("aria-controls"));
        if (!composer) return true;
        const open = !composer.classList.contains("is-open");
        composer.classList.toggle("is-open", open);
        document.querySelectorAll(`[data-composer-toggle][aria-controls="${composer.id}"]`).forEach((toggle) => {
            toggle.setAttribute("aria-expanded", String(open));
        });
        if (open) composer.querySelector("textarea")?.focus();
        return true;
    }

    // The composer opens by its anchor; the cursor goes straight to it.
    function onComposerOpen(target) {
        if (!target.closest("[data-composer-open]")) return false;
        window.setTimeout(() => {
            const message = document.getElementById("message");
            if (message) message.focus();
        }, 0);
        return true;
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
            setTheme(!root.classList.contains("dark"));
            return;
        }
        const choice = target.closest("[data-theme-choice]");
        if (choice) {
            setTheme(choice.dataset.themeChoice === "dark");
            return;
        }
        closeMoreActions(target.closest("details[data-more-actions]"));
        if (onMenuClick(target) || onComposerOpen(target) || onComposerToggle(target)) return;
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
            const more = document.querySelector("details[data-more-actions][open]");
            if (more && !(event.target instanceof Element && event.target.closest(".confirm"))) {
                more.open = false;
                more.querySelector("summary").focus();
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

    // The page is replaced only when what it shows has changed. A moved
    // instance clock (daylight saving) reloads it.
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
        } else if (version) {
            live.dataset.version = version;
            liveBefore.set(live, liveSnapshot(live));
        }
    }

    // Two refreshes in a row without an answer are said under the banner.
    function markStale(live, failed) {
        const failures = failed ? Number(live.dataset.failures || 0) + 1 : 0;
        live.dataset.failures = String(failures);
        const note = live.querySelector("[data-stale]");
        if (note) note.hidden = failures < 2;
    }

    // A read-only field as wide as its text: its size attribute counts
    // characters wider than the ones the page's font draws.
    function fitFieldsToText(scope) {
        scope.querySelectorAll("input[data-fit-text]").forEach((field) => {
            field.style.width = "0";
            field.style.width = `${field.scrollWidth}px`;
        });
    }

    document.body.addEventListener("htmx:afterSettle", (event) => {
        const settled = event.target;
        if (!(settled instanceof Element)) return;
        fitFieldsToText(settled);
        if (settled.hasAttribute("data-live")) lightChanges(settled);
        // A state set by hand comes back with a receipt: its cell lights up.
        document.querySelectorAll(".status-cell:has(.receipt):not([data-lit])").forEach((cell) => {
            cell.dataset.lit = "";
            flash(cell);
        });
        // An update posted from the side panel lights up at the head of it.
        const verb = event.detail.requestConfig && event.detail.requestConfig.verb;
        if (settled.id === "drawer-content" && verb === "post") {
            const newest = settled.querySelector(".timeline .moment");
            if (newest) flash(newest);
        }
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
        tickClock();
        window.setInterval(tickClock, 15000);
        startLiveRefresh();
        hideBrokenImages(document);
        document.fonts.ready.then(() => fitFieldsToText(document));
        document.querySelectorAll("form[data-autosubmit] [data-apply]").forEach((button) => {
            button.hidden = true;
        });
        const saved = document.querySelector("[data-scroll-into-view]");
        if (saved) saved.scrollIntoView({ block: "center" });
        lightPublished();
        takeListAddress();
    }

    // A page answering a form, the one showing a temporary password, takes
    // the address of its list: a reload then reads the list instead of
    // sending the form again.
    function takeListAddress() {
        const marker = document.querySelector("[data-replace-url]");
        if (marker) window.history.replaceState(null, "", marker.dataset.replaceUrl);
    }

    // Back on an event just published or updated, what was written lights
    // up once; the address loses its marker so a reload does not repeat it.
    function lightPublished() {
        const url = new URL(window.location.href);
        const published = url.searchParams.has("published");
        const posted = url.searchParams.has("posted");
        if (!published && !posted) return;
        const written = (published && document.querySelector(".article-text")) || document.querySelector(".timeline .moment");
        if (written) flash(written);
        url.searchParams.delete("published");
        url.searchParams.delete("posted");
        window.history.replaceState(null, "", url);
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", onReady);
    } else {
        onReady();
    }
})();

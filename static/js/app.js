// Behaviour shared by every page. Loaded deferred, after htmx on the pages
// that use it.
(function () {
    "use strict";

    const root = document.documentElement;
    const LIVE_REFRESH_MS = 60000;
    // A side panel that has not loaded by then offers to try again.
    const DRAWER_TIMEOUT_MS = 15000;
    // On a phone (frame.css sets the flag) a panel link opens its page
    // instead, which the phone's back button leaves.
    const isPhoneLayout = () =>
        getComputedStyle(root).getPropertyValue("--phone-layout").trim() === "1";
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

    // Opening the bell, or its page, marks the notices seen: the count goes
    // from both copies of the bell, the list keeps showing what was new.
    function markNoticesSeen() {
        const counts = document.querySelectorAll("[data-bell-count]");
        if (counts.length === 0) return;
        fetch("/notices/seen", {
            method: "POST",
            credentials: "same-origin",
            headers: { "X-CSRF-Token": csrfToken() },
        })
            .then((response) => {
                if (!response.ok) return;
                counts.forEach((count) => count.remove());
                document.querySelectorAll("[data-bell]").forEach((bell) => {
                    const label = bell.querySelector("[data-bell-label]");
                    if (label) label.textContent = bell.dataset.seenLabel;
                });
            })
            .catch(() => {});
    }

    document.querySelectorAll("details[data-bell]").forEach((bell) => {
        bell.addEventListener("toggle", () => {
            if (bell.open) markNoticesSeen();
        });
    });
    if (document.querySelector("[data-notices-seen]")) markNoticesSeen();

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

    // On a phone the masthead tucks away while the reader goes down the
    // page and comes back as soon as they go up. A few pixels of jitter
    // change nothing; the open menu keeps it in place.
    function watchMastOnScroll() {
        const mast = document.querySelector(".mast");
        if (!mast) return;
        let last = window.scrollY;
        window.addEventListener("scroll", () => {
            const y = window.scrollY;
            if (Math.abs(y - last) < 8) return;
            const down = y > last;
            last = y;
            const tuck = down && y > mast.offsetHeight && isPhoneLayout() && !menuIsOpen();
            mast.classList.toggle("is-tucked", tuck);
        }, { passive: true });
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
        const text = document.querySelector("[data-clock-24h]")
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
        // A masked field gives nothing to copy: it is unmasked for the copy.
        const fallback = () => {
            const masked = field.type === "password";
            if (masked) field.type = "text";
            if ("select" in field) field.select();
            else window.getSelection().selectAllChildren(field);
            if (document.execCommand("copy")) done();
            if (masked) field.type = "password";
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

    // The folded actions of an event close on any click outside them; a
    // menu opened inside another keeps the one around it open.
    function closeMoreActions(except) {
        document.querySelectorAll("details[data-more-actions][open]").forEach((menu) => {
            if (menu !== except && !menu.contains(except)) menu.open = false;
        });
    }

    // A menu entry that leads to a small form in the same panel, as "Folder"
    // under "New": the list gives way to the form until the menu closes.
    function onSwapToForm(target) {
        const entry = target.closest("[data-swap-to-form]");
        if (!entry) return false;
        const panel = entry.closest("[data-swap-panel]");
        panel?.classList.add("is-form");
        panel?.querySelector("input:not([type=hidden])")?.focus();
        return true;
    }

    // A composer folded behind a button, as in the side panel.
    function onComposerToggle(target) {
        const button = target.closest("[data-composer-toggle]");
        if (!button) return false;
        const composer = document.getElementById(button.getAttribute("aria-controls"));
        if (!composer) return true;
        const open = !composer.classList.contains("is-open");
        composer.classList.toggle("is-open", open);
        const toggles = [...document.querySelectorAll(`[data-composer-toggle][aria-controls="${composer.id}"]`)];
        toggles.forEach((toggle) => toggle.setAttribute("aria-expanded", String(open)));
        if (open) composer.querySelector("textarea, input:not([type=hidden])")?.focus();
        // Cancel hides itself: the keyboard goes back to what opened the form.
        else if (composer.contains(button)) toggles.find((toggle) => !composer.contains(toggle))?.focus();
        return true;
    }

    // The filters a phone folds behind a button, under the search.
    function onFiltersToggle(target) {
        const button = target.closest("[data-filters-toggle]");
        if (!button) return false;
        const more = document.getElementById(button.getAttribute("aria-controls"));
        if (!more) return true;
        const open = !more.classList.contains("is-open");
        more.classList.toggle("is-open", open);
        button.setAttribute("aria-expanded", String(open));
        return true;
    }

    // The count on that button follows the filters set besides the search;
    // the form itself is not redrawn when the results are.
    function countFilters(form) {
        const count = form.querySelector("[data-filters-count]");
        const more = form.querySelector(".filters-more");
        if (!count || !more) return;
        const set = [...more.querySelectorAll("select, input")].filter((field) => field.value !== "").length;
        count.textContent = String(set);
        count.hidden = set === 0;
    }

    // A form folded under its anchor, the composer or the review, opens by
    // it; the cursor goes straight to its text.
    function onComposerOpen(target) {
        const link = target.closest("[data-composer-open]");
        if (!link) return false;
        window.setTimeout(() => {
            const form = document.querySelector(link.getAttribute("href"));
            const text = form && form.querySelector("textarea");
            if (text) text.focus();
        }, 0);
        return true;
    }

    // A day an outage coloured on a service's row opens its panel on that
    // day's outages.
    function drawerUrl(link, target) {
        const day = target.closest("[data-outage-day]");
        if (!day || !link.contains(day)) return link.dataset.drawer;
        const url = new URL(link.dataset.drawer, window.location.href);
        url.searchParams.set("day", day.dataset.outageDay);
        return url.pathname + url.search;
    }

    // In the panel, the same day shows the outages found then; a second
    // click folds them away.
    function onOutageDay(target) {
        const day = target.closest("[data-outage-toggle]");
        if (!day) return false;
        const list = document.getElementById(day.getAttribute("aria-controls"));
        if (!list) return true;
        const show = day.getAttribute("aria-pressed") !== "true";
        for (const other of day.parentElement.querySelectorAll("[data-outage-toggle]")) {
            other.setAttribute("aria-pressed", "false");
        }
        day.setAttribute("aria-pressed", String(show));
        for (const item of list.querySelectorAll("[data-outage-days]")) {
            item.hidden = !show || !item.dataset.outageDays.split(" ").includes(day.dataset.outageToggle);
        }
        list.hidden = !show;
        return true;
    }

    function onDrawerClick(target, event) {
        const link = target.closest("[data-drawer]");
        if (link && !event.metaKey && !event.ctrlKey && !event.shiftKey && event.button === 0) {
            if (!isPhoneLayout() && openDrawer(drawerUrl(link, target), link)) {
                event.preventDefault();
            }
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
        if (onMenuClick(target) || onSwapToForm(target) || onComposerOpen(target) || onComposerToggle(target) || onFiltersToggle(target)) return;
        if (target.closest("[data-toast-close]")) {
            clearToast();
            return;
        }
        if (onOutageDay(target) || onDrawerClick(target, event) || onConfirmClick(target)) return;
        const copy = target.closest("[data-copy]");
        if (copy) copyValue(copy);
    });

    // A panel left open when the screen becomes a phone's, by turning it
    // upright, closes rather than filling the screen.
    window.addEventListener("resize", () => {
        if (drawerIsOpen() && isPhoneLayout()) closeDrawer();
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
            // The innermost open menu closes first, then the one around it.
            const more = [...document.querySelectorAll("details[data-more-actions][open]")].pop();
            if (more && !(event.target instanceof Element && event.target.closest(".confirm"))) {
                more.open = false;
                more.querySelector("summary").focus();
                return;
            }
            if (event.target instanceof Element) cancelQuestion(event.target);
            return;
        }
        if (event.key === "Tab" && drawerIsOpen()) trapDrawerFocus(event);

    });

    // Safari scrolls the page to its top when a menu of the sticky masthead
    // takes the focus, misled by the room kept above focused fields
    // (scroll-padding-top, WebKit bug 272799): a click focuses it in place.
    document.addEventListener("mousedown", (event) => {
        const summary = event.target instanceof Element ? event.target.closest(".mast summary") : null;
        if (!summary) return;
        event.preventDefault();
        summary.focus({ preventScroll: true });
    });

    document.addEventListener("change", (event) => {
        const target = event.target;
        if (!(target instanceof Element)) return;
        const picked = target.matches("input[type=file]") && target.closest("label")?.querySelector("[data-file-name]");
        if (picked && target.files[0]) picked.textContent = target.files[0].name;
        const upload = target.closest("form[data-submit-on-change]");
        if (upload && target.matches('input[type="file"]') && target.files.length > 0) {
            upload.requestSubmit();
            return;
        }
        const filters = target.closest("form.filters");
        if (filters) countFilters(filters);
    });

    // A form whose changes wait for its button: leaving before it is sent
    // asks first, in the browser's own words, and its note says so.
    function showUnsaved(form, unsaved) {
        if (unsaved) form.dataset.unsaved = "1";
        else delete form.dataset.unsaved;
        form.querySelectorAll("[data-unsaved-note]").forEach((note) => (note.hidden = !unsaved));
    }

    function markUnsaved(event) {
        const form = event.target instanceof Element ? event.target.closest("form[data-warn-unsaved]") : null;
        if (form) showUnsaved(form, true);
    }

    document.addEventListener("input", markUnsaved);
    document.addEventListener("change", markUnsaved);
    document.addEventListener("submit", (event) => {
        if (event.target instanceof HTMLFormElement) showUnsaved(event.target, false);
    });
    // A menu closed without saving forgets what was changed in its forms.
    document.addEventListener(
        "toggle",
        (event) => {
            const menu = event.target;
            if (!(menu instanceof HTMLDetailsElement) || menu.open) return;
            if (!menu.hasAttribute("data-more-actions")) return;
            menu.querySelectorAll("[data-swap-panel]").forEach((panel) => panel.classList.remove("is-form"));
            menu.querySelectorAll("form[data-warn-unsaved]").forEach((form) => {
                form.reset();
                showUnsaved(form, false);
            });
        },
        true,
    );
    window.addEventListener("beforeunload", (event) => {
        if (document.querySelector("form[data-warn-unsaved][data-unsaved]")) event.preventDefault();
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
        watchMastOnScroll();
        syncThemeButtons();
        tickClock();
        window.setInterval(tickClock, 15000);
        startLiveRefresh();
        hideBrokenImages(document);
        document.fonts.ready.then(() => fitFieldsToText(document));
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

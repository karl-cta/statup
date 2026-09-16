// Custom select over a native <select data-custom-select>. The native element
// stays in the form for submission and change events; the list is an ARIA
// listbox portaled to the body so no card clips it. An option with a
// data-tone shows the matching status dot.
(function () {
    "use strict";

    const CARET =
        '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" ' +
        'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5"/></svg>';
    const MAX_HEIGHT = 280;
    const MOVES = { ArrowDown: 1, ArrowUp: -1 };
    let counter = 0;

    function optionContent(option) {
        const parts = [];
        if (option.dataset.tone) {
            const dot = document.createElement("span");
            dot.className = "dot";
            dot.dataset.tone = option.dataset.tone;
            dot.setAttribute("aria-hidden", "true");
            parts.push(dot);
        }
        const text = document.createElement("span");
        text.className = "cs-label";
        text.textContent = option.textContent;
        parts.push(text);
        return parts;
    }

    function buildTrigger(listId) {
        const trigger = document.createElement("button");
        trigger.type = "button";
        trigger.className = "cs-trigger";
        trigger.setAttribute("role", "combobox");
        trigger.setAttribute("aria-haspopup", "listbox");
        trigger.setAttribute("aria-expanded", "false");
        trigger.setAttribute("aria-controls", listId);
        const face = document.createElement("span");
        face.className = "cs-face";
        const caret = document.createElement("span");
        caret.className = "cs-caret";
        caret.innerHTML = CARET;
        trigger.append(face, caret);
        return { trigger, face };
    }

    function buildOption(option, id) {
        const item = document.createElement("li");
        item.className = "cs-option";
        item.id = id;
        item.setAttribute("role", "option");
        item.dataset.value = option.value;
        if (option.disabled) {
            item.setAttribute("aria-disabled", "true");
            item.classList.add("cs-option-disabled");
        }
        item.append(...optionContent(option));
        return item;
    }

    function buildList(native, baseId, label) {
        const list = document.createElement("ul");
        list.className = "cs-list";
        list.id = `${baseId}-list`;
        list.setAttribute("role", "listbox");
        list.setAttribute("tabindex", "-1");
        if (label) list.setAttribute("aria-labelledby", label.id);
        list.hidden = true;
        list.csOwner = native;
        Array.from(native.options).forEach((option, index) => {
            list.append(buildOption(option, `${baseId}-opt-${index}`));
        });
        return list;
    }

    class CustomSelect {
        constructor(native) {
            counter += 1;
            if (!native.id) native.id = `cs-${counter}`;
            this.native = native;
            this.isOpen = false;
            this.active = -1;
            this.typed = "";
            this.typedTimer = 0;
            this.wrap = document.createElement("div");
            this.wrap.className = "cs-wrap";
            const { trigger, face } = buildTrigger(`${native.id}-list`);
            this.trigger = trigger;
            this.face = face;
            const label = this.labelWith(native.labels && native.labels[0]);
            if (native.disabled) {
                this.wrap.classList.add("cs-disabled");
                trigger.disabled = true;
            }
            this.list = buildList(native, native.id, label);
            this.onOutside = (event) => {
                if (!this.wrap.contains(event.target) && !this.list.contains(event.target)) this.close(false);
            };
            // Scrolling the page moves the trigger away, so the list closes;
            // scrolling the list itself does not.
            this.onViewport = (event) => {
                if (event.target !== this.list) this.close(false);
            };
            this.mount();
            this.bind();
            this.sync();
        }

        // The native select is hidden: a click on its label lands here.
        labelWith(label) {
            if (!label) {
                const name = this.native.getAttribute("aria-label");
                if (name) this.trigger.setAttribute("aria-label", name);
                return null;
            }
            if (!label.id) label.id = `${this.native.id}-label`;
            this.trigger.setAttribute("aria-labelledby", label.id);
            label.addEventListener("click", (event) => {
                event.preventDefault();
                this.trigger.focus();
            });
            return label;
        }

        mount() {
            const native = this.native;
            native.parentNode.insertBefore(this.wrap, native);
            this.wrap.append(native, this.trigger);
            document.body.append(this.list);
            native.classList.add("cs-native");
            native.tabIndex = -1;
            native.setAttribute("aria-hidden", "true");
        }

        items() {
            return Array.from(this.list.children);
        }

        sync() {
            const option = this.native.options[this.native.selectedIndex];
            this.face.replaceChildren(...(option ? optionContent(option) : []));
            this.items().forEach((item) => {
                item.setAttribute("aria-selected", String(item.dataset.value === this.native.value));
            });
        }

        selectedIndex() {
            const index = this.items().findIndex((item) => item.dataset.value === this.native.value);
            return Math.max(index, 0);
        }

        setActive(index) {
            const all = this.items();
            if (all.length === 0) return;
            this.active = Math.min(Math.max(index, 0), all.length - 1);
            all.forEach((item, i) => item.classList.toggle("cs-active", i === this.active));
            const item = all[this.active];
            this.trigger.setAttribute("aria-activedescendant", item.id);
            const list = this.list;
            if (item.offsetTop < list.scrollTop) {
                list.scrollTop = item.offsetTop;
            } else if (item.offsetTop + item.offsetHeight > list.scrollTop + list.clientHeight) {
                list.scrollTop = item.offsetTop + item.offsetHeight - list.clientHeight;
            }
        }

        // Below the trigger, or above it when there is more room there.
        place() {
            const rect = this.trigger.getBoundingClientRect();
            const below = window.innerHeight - rect.bottom;
            const above = rect.top;
            const upward = below < Math.min(MAX_HEIGHT, 200) && above > below;
            const style = this.list.style;
            style.left = `${rect.left}px`;
            style.minWidth = `${rect.width}px`;
            style.top = upward ? "" : `${rect.bottom + 4}px`;
            style.bottom = upward ? `${window.innerHeight - rect.top + 4}px` : "";
            const room = upward ? above : below;
            style.maxHeight = `${Math.max(120, Math.min(MAX_HEIGHT, room - 16))}px`;
        }

        show() {
            if (this.isOpen || this.native.disabled) return;
            this.isOpen = true;
            this.list.hidden = false;
            this.place();
            this.trigger.setAttribute("aria-expanded", "true");
            this.wrap.classList.add("cs-open");
            this.setActive(this.selectedIndex());
            document.addEventListener("pointerdown", this.onOutside, true);
            window.addEventListener("resize", this.onViewport, true);
            window.addEventListener("scroll", this.onViewport, true);
        }

        close(focus) {
            if (!this.isOpen) return;
            this.isOpen = false;
            this.list.hidden = true;
            this.trigger.setAttribute("aria-expanded", "false");
            this.trigger.removeAttribute("aria-activedescendant");
            this.wrap.classList.remove("cs-open");
            document.removeEventListener("pointerdown", this.onOutside, true);
            window.removeEventListener("resize", this.onViewport, true);
            window.removeEventListener("scroll", this.onViewport, true);
            if (focus) this.trigger.focus();
        }

        commit(index) {
            const item = this.items()[index];
            if (!item || item.getAttribute("aria-disabled") === "true") return;
            const changed = item.dataset.value !== this.native.value;
            this.native.value = item.dataset.value;
            this.sync();
            this.close(true);
            if (changed) this.native.dispatchEvent(new Event("change", { bubbles: true }));
        }

        typeAhead(key) {
            if (!this.isOpen) this.show();
            window.clearTimeout(this.typedTimer);
            this.typed += key.toLowerCase();
            this.typedTimer = window.setTimeout(() => {
                this.typed = "";
            }, 500);
            const all = this.items();
            for (let step = 1; step <= all.length; step += 1) {
                const index = (this.active + step) % all.length;
                if (all[index].textContent.trim().toLowerCase().startsWith(this.typed)) {
                    this.setActive(index);
                    return;
                }
            }
        }

        onKey(event) {
            const key = event.key;
            if (key in MOVES) {
                event.preventDefault();
                if (this.isOpen) this.setActive(this.active + MOVES[key]);
                else this.show();
            } else if ((key === "Home" || key === "End") && this.isOpen) {
                event.preventDefault();
                this.setActive(key === "Home" ? 0 : this.items().length - 1);
            } else if (key === "Enter" || key === " ") {
                event.preventDefault();
                if (this.isOpen) this.commit(this.active);
                else this.show();
            } else if (key === "Escape" && this.isOpen) {
                event.preventDefault();
                event.stopPropagation();
                this.close(true);
            } else if (key === "Tab") {
                this.close(false);
            } else if (key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
                event.preventDefault();
                this.typeAhead(key);
            }
        }

        bind() {
            this.trigger.addEventListener("click", () => (this.isOpen ? this.close(true) : this.show()));
            this.trigger.addEventListener("keydown", (event) => this.onKey(event));
            // Focus stays on the trigger; the choice is made on release, so a
            // finger can scroll a long list without picking what it touched.
            this.list.addEventListener("pointerdown", (event) => {
                if (event.target.closest('[role="option"]')) event.preventDefault();
            });
            this.list.addEventListener("click", (event) => {
                const item = event.target.closest('[role="option"]');
                if (item) this.commit(this.items().indexOf(item));
            });
            this.list.addEventListener("pointermove", (event) => {
                const item = event.target.closest('[role="option"]');
                if (item) this.setActive(this.items().indexOf(item));
            });
            // Another script that puts a value back asks for a redraw without a
            // change event, which would read as a new choice.
            this.native.addEventListener("cs:sync", () => this.sync());
            this.native.addEventListener("change", (event) => {
                if (event.isTrusted) this.sync();
            });
        }
    }

    function enhance(native) {
        if (native.dataset.csReady) return;
        native.dataset.csReady = "1";
        new CustomSelect(native);
    }

    // Lists whose select left the page go with it.
    function prune() {
        document.querySelectorAll("body > .cs-list").forEach((list) => {
            if (!list.csOwner || !list.csOwner.isConnected) list.remove();
        });
    }

    function enhanceAll(scope) {
        prune();
        scope.querySelectorAll("select[data-custom-select]").forEach(enhance);
    }

    if (window.htmx) {
        window.htmx.onLoad(enhanceAll);
    } else {
        enhanceAll(document);
    }
})();

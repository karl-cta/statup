// A moment picked on a calendar with a time typed freely: the window of a
// maintenance, with a duration and the end it leads to, or the real start
// of an incident declared late (data-past). Times are the instance's wall
// clock, held in Date objects read through their UTC fields so the
// browser's own zone never shifts them.
(function () {
    "use strict";

    const copyNode = document.getElementById("schedule-copy");
    const roots = document.querySelectorAll("[data-schedule]");
    if (!copyNode || !roots.length) return;

    const copy = JSON.parse(copyNode.textContent);
    roots.forEach((root) => setup(root));

    function setup(root) {
        const lang = document.documentElement.lang || "en";
        const past = root.hasAttribute("data-past");
        const form = root.closest("form");
        const startValue = root.querySelector("[data-start-value]");
        const endValue = root.querySelector("[data-end-value]");
        const trigger = root.querySelector("[data-date-trigger]");
        const dateText = root.querySelector("[data-date-text]");
        const time = root.querySelector("[data-time]");
        const timeField = time.closest(".schedule-time");
        const timeList = root.querySelector("[data-time-list]");
        const startRow = root.querySelector(".schedule-start");
        const hours = root.querySelector("[data-hours]");
        const minutes = root.querySelector("[data-minutes]");
        const endText = root.querySelector("[data-end-text]");
        const calendar = root.querySelector("[data-calendar]");
        const locked = trigger.disabled;
        const timed = Boolean(hours && minutes && endText && endValue);

        const MINUTE = 60000;
        const DAY = 86400000;
        const loadedAt = Date.now();
        const pad = (n) => String(n).padStart(2, "0");

        function parseWall(value) {
            const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(value || "");
            return m ? new Date(Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5])) : null;
        }

        const formatWall = (d) =>
            `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}T${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
        const clockOf = (d) => `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
        const dayOf = (d) => new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()));
        const monthOf = (d) => new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1));
        const addDays = (d, n) => new Date(d.getTime() + n * DAY);
        const sameDay = (a, b) => a && b && a.getTime() === b.getTime();
        const upperFirst = (text) => text.charAt(0).toUpperCase() + text.slice(1);

        const pageNow = parseWall(root.dataset.now) || new Date();
        // The instance clock, carried forward from when the page was drawn.
        const now = () => new Date(pageNow.getTime() + (Date.now() - loadedAt));
        const today = () => dayOf(now());

        const format = (options) => new Intl.DateTimeFormat(lang, { ...options, timeZone: "UTC" });
        const dayWithYear = format({ weekday: "short", day: "numeric", month: "short", year: "numeric" });
        const dayWithoutYear = format({ weekday: "short", day: "numeric", month: "short" });
        const dayInFull = format({ weekday: "long", day: "numeric", month: "long", year: "numeric" });
        const monthTitle = format({ month: "long", year: "numeric" });
        const weekdayName = format({ weekday: "short" });

        let day = null;
        let shownMonth = null;

        // "20", "20h", "8h30", "20:30", "2030": minutes into the day, null when
        // empty, NaN when unreadable.
        function parseTime(text) {
            const clean = text.trim().toLowerCase().replace(/\s+/g, "").replace(/[h.]/g, ":");
            if (!clean) return null;
            let h;
            let m;
            const parts = /^(\d{1,2})(?::(\d{2})?)?$/.exec(clean);
            if (parts) {
                h = Number(parts[1]);
                m = parts[2] ? Number(parts[2]) : 0;
            } else if (/^\d{3,4}$/.test(clean)) {
                h = Number(clean.slice(0, -2));
                m = Number(clean.slice(-2));
            } else {
                return Number.NaN;
            }
            return h < 24 && m < 60 ? h * 60 + m : Number.NaN;
        }

        const digits = (input) => Number(input.value.replace(/\D/g, "")) || 0;
        const durationMinutes = () => (timed ? digits(hours) * 60 + digits(minutes) : 0);

        // The start typed in, or null for right away, with the reason it
        // cannot be read.
        function readStart() {
            if (locked) return { start: parseWall(startValue.value), error: "" };
            if (!day) return { start: null, error: "" };
            const minutesIn = parseTime(time.value);
            if (Number.isNaN(minutesIn)) return { start: null, error: copy.time_invalid };
            if (minutesIn === null) return { start: null, error: copy.time_required };
            return { start: new Date(day.getTime() + minutesIn * MINUTE), error: "" };
        }

        function describeEnd(end) {
            const at = clockOf(end);
            const offset = Math.round((dayOf(end).getTime() - today().getTime()) / DAY);
            if (offset === 0) return copy.end_today.replace("{time}", at);
            if (offset === 1) return copy.end_tomorrow.replace("{time}", at);
            const label = end.getUTCFullYear() === today().getUTCFullYear() ? dayWithoutYear : dayWithYear;
            return copy.end_on.replace("{day}", upperFirst(label.format(end))).replace("{time}", at);
        }

        // "Now" until a day is picked; the time only matters once there is one.
        function dateLabel() {
            if (!day) return copy.now;
            return sameDay(day, today()) ? copy.today : upperFirst(dayWithYear.format(day));
        }

        function setValue(input, value) {
            if (input.value === value) return;
            input.value = value;
            input.dispatchEvent(new Event("change", { bubbles: true }));
        }

        function sync() {
            const { start, error } = readStart();
            time.setCustomValidity(error);
            const total = durationMinutes();
            const end = total ? new Date((start || now()).getTime() + total * MINUTE) : null;
            dateText.textContent = dateLabel();
            timeField.hidden = !day;
            startRow.classList.toggle("is-timed", Boolean(day));
            if (!locked) setValue(startValue, start ? formatWall(start) : "");
            if (!timed) return;
            endText.textContent = end ? describeEnd(end) : copy.no_end;
            endText.classList.toggle("is-empty", !end);
            setValue(endValue, end ? formatWall(end) : "");
        }

        function restore() {
            const start = parseWall(startValue.value);
            if (start) {
                day = dayOf(start);
                time.value = clockOf(start);
            }
            const end = timed ? parseWall(endValue.value) : null;
            const total = end ? Math.round((end.getTime() - (start || now()).getTime()) / MINUTE) : 0;
            if (total > 0) {
                hours.value = Math.floor(total / 60) || "";
                minutes.value = total % 60 || "";
            }
        }

        // Calendar.

        const svgNode = (tag, attrs) => {
            const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
            Object.entries(attrs).forEach(([key, value]) => node.setAttribute(key, value));
            return node;
        };

        function chevron(path) {
            const icon = svgNode("svg", {
                viewBox: "0 0 24 24",
                fill: "none",
                stroke: "currentColor",
                "stroke-width": "2",
                "stroke-linecap": "round",
                "stroke-linejoin": "round",
                "aria-hidden": "true",
            });
            icon.append(svgNode("path", { d: path }));
            return icon;
        }

        function navButton(label, path, months) {
            const button = document.createElement("button");
            button.type = "button";
            button.className = "cal-nav";
            button.setAttribute("aria-label", label);
            button.append(chevron(path));
            button.addEventListener("click", () => showMonth(addMonths(shownMonth, months)));
            return button;
        }

        const addMonths = (d, n) => new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + n, 1));
        // Ahead of today for work to come, up to today for what already began.
        const firstAllowed = () => (past ? null : today());
        const lastAllowed = () => (past ? today() : null);
        const outOfRange = (date) =>
            Boolean((firstAllowed() && date < firstAllowed()) || (lastAllowed() && date > lastAllowed()));

        function clamp(date) {
            if (firstAllowed() && date < firstAllowed()) return firstAllowed();
            if (lastAllowed() && date > lastAllowed()) return lastAllowed();
            return date;
        }

        function header() {
            const bar = document.createElement("div");
            bar.className = "cal-head";
            const title = document.createElement("p");
            title.className = "cal-title";
            title.setAttribute("aria-live", "polite");
            title.textContent = upperFirst(monthTitle.format(shownMonth));
            const prev = navButton(copy.prev_month, "m15 18-6-6 6-6", -1);
            prev.disabled = Boolean(firstAllowed()) && shownMonth.getTime() <= monthOf(firstAllowed()).getTime();
            const next = navButton(copy.next_month, "m9 18 6-6-6-6", 1);
            next.disabled = Boolean(lastAllowed()) && shownMonth.getTime() >= monthOf(lastAllowed()).getTime();
            bar.append(prev, title, next);
            return bar;
        }

        function weekdays() {
            const row = document.createElement("tr");
            // 1 January 2024 was a Monday: weeks start on Monday.
            for (let i = 0; i < 7; i += 1) {
                const cell = document.createElement("th");
                cell.scope = "col";
                const monday = new Date(Date.UTC(2024, 0, 1 + i));
                cell.textContent = upperFirst(weekdayName.format(monday).replace(".", ""));
                row.append(cell);
            }
            const head = document.createElement("thead");
            head.append(row);
            return head;
        }

        function dayButton(date) {
            const button = document.createElement("button");
            button.type = "button";
            button.className = "cal-day";
            button.tabIndex = -1;
            button.dataset.day = String(date.getTime());
            button.textContent = String(date.getUTCDate());
            button.setAttribute("aria-label", upperFirst(dayInFull.format(date)));
            button.setAttribute("aria-pressed", String(Boolean(sameDay(date, day))));
            if (sameDay(date, today())) button.setAttribute("aria-current", "date");
            button.disabled = outOfRange(date);
            return button;
        }

        function body() {
            const rows = document.createElement("tbody");
            const first = shownMonth;
            const lead = (first.getUTCDay() + 6) % 7;
            const count = new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth() + 1, 0)).getUTCDate();
            let row = document.createElement("tr");
            for (let i = 0; i < lead; i += 1) row.append(document.createElement("td"));
            for (let n = 1; n <= count; n += 1) {
                if (row.children.length === 7) {
                    rows.append(row);
                    row = document.createElement("tr");
                }
                const cell = document.createElement("td");
                cell.append(dayButton(new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth(), n))));
                row.append(cell);
            }
            while (row.children.length < 7) row.append(document.createElement("td"));
            rows.append(row);
            return rows;
        }

        function footer() {
            const bar = document.createElement("div");
            bar.className = "cal-foot";
            const reset = document.createElement("button");
            reset.type = "button";
            reset.className = "text-action";
            reset.textContent = past ? copy.now : copy.start_right_away;
            reset.addEventListener("click", () => {
                day = null;
                time.value = "";
                time.removeAttribute("aria-invalid");
                closeCalendar(true);
                sync();
            });
            bar.append(reset);
            return bar;
        }

        function render() {
            const table = document.createElement("table");
            table.className = "cal-grid";
            table.append(weekdays(), body());
            calendar.replaceChildren(header(), table, footer());
        }

        function focusDay(date) {
            const target = calendar.querySelector(`[data-day="${date.getTime()}"]`);
            if (!target) return;
            calendar.querySelectorAll(".cal-day").forEach((button) => {
                button.tabIndex = -1;
            });
            target.tabIndex = 0;
            target.focus();
        }

        function showMonth(month, focus) {
            shownMonth = month;
            render();
            const pick = focus || day || today();
            const inMonth = monthOf(pick).getTime() === month.getTime() ? pick : month;
            focusDay(clamp(inMonth));
        }

        function openCalendar() {
            calendar.hidden = false;
            trigger.setAttribute("aria-expanded", "true");
            showMonth(monthOf(day || today()));
            calendar.scrollIntoView({ block: "nearest" });
        }

        function closeCalendar(returnFocus) {
            if (calendar.hidden) return;
            calendar.hidden = true;
            trigger.setAttribute("aria-expanded", "false");
            if (returnFocus) trigger.focus();
        }

        function pick(date) {
            day = date;
            closeCalendar(false);
            sync();
            time.focus();
            openTimes();
        }

        // Times every half hour, under a field that still takes any time typed.

        const SLOT = 30;
        const clockText = (value) => `${pad(Math.floor(value / 60))}:${pad(value % 60)}`;

        function slots() {
            const all = Array.from({ length: (24 * 60) / SLOT }, (_, i) => i * SLOT);
            if (!sameDay(day, today())) return all;
            const current = now().getUTCHours() * 60 + now().getUTCMinutes();
            return all.filter((value) => (past ? value <= current : value > current));
        }

        function activeOption() {
            return timeList.querySelector('[aria-selected="true"]');
        }

        function setActive(option) {
            const current = activeOption();
            if (current) current.setAttribute("aria-selected", "false");
            if (!option) {
                time.removeAttribute("aria-activedescendant");
                return;
            }
            option.setAttribute("aria-selected", "true");
            time.setAttribute("aria-activedescendant", option.id);
            const top = option.offsetTop - timeList.clientHeight / 2 + option.offsetHeight / 2;
            timeList.scrollTop = Math.max(0, top);
        }

        // The slot at or just after what is typed, or after the current time
        // of day while nothing is.
        function nearestOption() {
            let target = parseTime(time.value);
            if (target === null || Number.isNaN(target)) target = now().getUTCHours() * 60 + now().getUTCMinutes();
            const options = Array.from(timeList.children);
            return options.find((option) => Number(option.dataset.value) >= target) || options[options.length - 1] || null;
        }

        function openTimes() {
            const options = slots().map((value) => {
                const option = document.createElement("li");
                option.id = `${timeList.id}-${value}`;
                option.className = "time-option";
                option.setAttribute("role", "option");
                option.setAttribute("aria-selected", "false");
                option.dataset.value = String(value);
                option.textContent = clockText(value);
                return option;
            });
            timeList.replaceChildren(...options);
            if (!options.length) return;
            timeList.hidden = false;
            time.setAttribute("aria-expanded", "true");
            setActive(nearestOption());
        }

        function closeTimes() {
            timeList.hidden = true;
            time.setAttribute("aria-expanded", "false");
            setActive(null);
        }

        function chooseTime(option) {
            time.value = option.textContent;
            time.removeAttribute("aria-invalid");
            closeTimes();
            sync();
        }

        function onTimeKey(event) {
            if (event.key === "Escape" && !timeList.hidden) {
                event.preventDefault();
                closeTimes();
                return;
            }
            if (event.key === "Enter" && !timeList.hidden && activeOption()) {
                event.preventDefault();
                chooseTime(activeOption());
                return;
            }
            if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
            event.preventDefault();
            if (timeList.hidden) {
                openTimes();
                return;
            }
            const current = activeOption();
            const next = event.key === "ArrowDown" ? current?.nextElementSibling : current?.previousElementSibling;
            setActive(next || current);
        }

        const KEY_STEPS = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };

        function moveFocus(event, current) {
            let target = null;
            if (event.key in KEY_STEPS) target = addDays(current, KEY_STEPS[event.key]);
            else if (event.key === "Home") target = addDays(current, -((current.getUTCDay() + 6) % 7));
            else if (event.key === "End") target = addDays(current, 6 - ((current.getUTCDay() + 6) % 7));
            else if (event.key === "PageUp" || event.key === "PageDown") {
                const month = addMonths(current, event.key === "PageUp" ? -1 : 1);
                const last = new Date(Date.UTC(month.getUTCFullYear(), month.getUTCMonth() + 1, 0)).getUTCDate();
                target = new Date(Date.UTC(month.getUTCFullYear(), month.getUTCMonth(), Math.min(current.getUTCDate(), last)));
            }
            if (!target) return;
            event.preventDefault();
            target = clamp(target);
            if (monthOf(target).getTime() !== shownMonth.getTime()) showMonth(monthOf(target), target);
            else focusDay(target);
        }

        // Wiring.

        if (!locked) {
            trigger.addEventListener("click", () => (calendar.hidden ? openCalendar() : closeCalendar(true)));
            calendar.addEventListener("click", (event) => {
                const button = event.target.closest(".cal-day");
                if (button && !button.disabled) pick(new Date(Number(button.dataset.day)));
            });
            calendar.addEventListener("keydown", (event) => {
                if (event.key === "Escape") {
                    event.preventDefault();
                    closeCalendar(true);
                    return;
                }
                const button = event.target.closest(".cal-day");
                if (button) moveFocus(event, new Date(Number(button.dataset.day)));
            });
            document.addEventListener("pointerdown", (event) => {
                if (!calendar.hidden && !calendar.contains(event.target) && !trigger.contains(event.target)) {
                    closeCalendar(false);
                }
            });
            root.addEventListener("focusout", (event) => {
                const next = event.relatedTarget;
                if (next instanceof Node && !calendar.contains(next) && next !== trigger) closeCalendar(false);
            });
            time.addEventListener("input", () => {
                time.removeAttribute("aria-invalid");
                if (!timeList.hidden) setActive(nearestOption());
                sync();
            });
            time.addEventListener("click", () => {
                if (timeList.hidden) openTimes();
            });
            time.addEventListener("keydown", onTimeKey);
            time.addEventListener("blur", () => {
                closeTimes();
                const value = parseTime(time.value);
                if (Number.isNaN(value)) time.setAttribute("aria-invalid", "true");
                else if (value !== null) time.value = clockText(value);
            });
            // A press on the list must not blur the field before the choice lands.
            timeList.addEventListener("mousedown", (event) => event.preventDefault());
            timeList.addEventListener("click", (event) => {
                const option = event.target.closest(".time-option");
                if (option) chooseTime(option);
            });
        }

        if (timed) {
            [hours, minutes].forEach((input) => {
                input.addEventListener("input", () => {
                    input.value = input.value.replace(/\D/g, "");
                    sync();
                });
            });
            // Ninety minutes read better as 1 h 30.
            minutes.addEventListener("blur", () => {
                const total = durationMinutes();
                if (digits(minutes) < 60) return;
                hours.value = Math.floor(total / 60) || "";
                minutes.value = total % 60 || "";
            });
        }

        // "Right away" and an end counted from now are settled when sent.
        form.addEventListener("submit", sync);

        restore();
        sync();
    }
})();

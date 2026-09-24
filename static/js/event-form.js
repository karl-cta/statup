// Event form: rows follow the chosen kind, the line under the services says
// what the public page will show, the title writes itself until typed in,
// and a saved template fills the form.
(function () {
    "use strict";

    const form = document.querySelector("[data-event-form]");
    if (!form) return;

    // The rows that do not apply to the event's kind are hidden by the
    // stylesheet; their fields must leave the request too.
    function settleRows() {
        form.querySelectorAll(".reveal-row").forEach((row) => {
            const open = row.classList.contains("is-open");
            row.querySelectorAll("input, select, textarea").forEach((field) => {
                field.disabled = !open;
                if (field.hasAttribute("data-required")) field.required = open;
            });
        });
    }

    // An edited event keeps its kind: the rows are settled once and the
    // wording of a new event is not needed.
    const copyNode = document.getElementById("event-form-copy");
    if (form.hasAttribute("data-editing") || !copyNode) {
        settleRows();
        return;
    }

    const copy = JSON.parse(copyNode.textContent);
    const title = form.querySelector("#title");
    const suggestions = form.querySelector("[data-suggestions]");
    const start = form.querySelector("#planned_start");
    let titleTouched = title.value.trim() !== "";

    const checked = (name) => {
        const input = form.querySelector(`input[name="${name}"]:checked`);
        return input ? input.value : "";
    };

    const fill = (text, services) => text.replace("{services}", services);

    function serviceNames() {
        return Array.from(form.querySelectorAll('input[name="service_ids"]:checked'), (box) => box.dataset.serviceName);
    }

    // Up to three names are written out; past that a count reads better.
    function joinNames(names) {
        if (names.length > 3) return copy.services_many.replace("{n}", String(names.length));
        if (names.length === 1) return names[0];
        return `${names.slice(0, -1).join(", ")} ${copy.and} ${names[names.length - 1]}`;
    }

    // A row opened by a choice slides in; one open on arrival does not.
    form.addEventListener("animationend", (event) => event.target.classList.remove("is-revealing"));

    function setOpen(row, open) {
        if (open && !row.classList.contains("is-open")) row.classList.add("is-revealing");
        row.classList.toggle("is-open", open);
        row.querySelectorAll("input, select, textarea").forEach((field) => {
            field.disabled = !open;
            if (field.hasAttribute("data-required")) field.required = open;
        });
    }

    // What the public page will show, or null when nothing on it changes.
    function outcome(kind, severity, names) {
        if (kind === "publication" || (kind === "incident" && !severity)) return null;
        if (names.length === 0) return { text: copy.no_services, tone: "" };
        const services = joinNames(names);
        if (kind === "maintenance") {
            if (form.querySelector('input[name="keeps_services_up"]:checked')) return { text: fill(copy.maintenance_up, services), tone: "ok" };
            const key = start && start.value ? "maintenance_planned" : "maintenance_now";
            return { text: fill(copy[key], services), tone: "info" };
        }
        // Declared once under watch, the incident leaves its services as they are.
        if (checked("opening_step") === "monitoring") return { text: fill(copy.incident_monitoring, services), tone: "ok" };
        const tone = severity === "critical" ? "crit" : severity;
        return { text: fill(copy[`incident_${severity}`], services), tone };
    }

    function proposeTitle(kind, severity, names) {
        if (titleTouched) return;
        const services = names.length ? joinNames(names) : "";
        let lead = "";
        // French elides before a vowel: "Maintenance d'Internet".
        if (kind === "maintenance") lead = /^[aeiouyàâäéèêëîïôöùûü]/i.test(services) ? copy.title_maintenance_vowel : copy.title_maintenance;
        else if (kind === "incident" && severity) lead = copy[`title_incident_${severity}`];
        const proposal = lead && services ? fill(lead, services).slice(0, 200) : "";
        if (proposal !== title.value) title.value = proposal;
    }

    function update() {
        const kind = checked("kind") || "incident";
        const severity = checked("severity");
        const names = serviceNames();
        form.querySelectorAll(".reveal-row").forEach((row) => {
            setOpen(row, row.dataset.forKind === kind);
        });

        const result = outcome(kind, severity, names);
        form.querySelector("[data-consequence-row]").hidden = !result;
        if (result) {
            form.querySelector("[data-consequence]").textContent = result.text;
            const dot = form.querySelector("[data-consequence-dot]");
            dot.hidden = !result.tone;
            if (result.tone) dot.dataset.tone = result.tone;
        }

        proposeTitle(kind, severity, names);
        // A field left for the button fires "change" between the press and
        // the release; Safari drops the click if the label's text node is
        // replaced in between.
        const label = form.querySelector("[data-submit-label]");
        const submitText = copy[`submit_${kind}`] || copy.submit_incident;
        if (label.textContent !== submitText) label.textContent = submitText;
    }

    function check(name, value) {
        if (!value) return;
        const input = form.querySelector(`input[name="${name}"][value="${value}"]`);
        if (input) input.checked = true;
    }

    function closeSuggestions() {
        if (suggestions) suggestions.replaceChildren();
        title.setAttribute("aria-expanded", "false");
    }

    function showTemplateError() {
        const box = document.createElement("div");
        box.className = "form-error mt-2";
        const text = document.createElement("p");
        text.textContent = copy.template_error;
        box.append(text);
        suggestions.replaceChildren(box);
    }

    function applyTemplate(id) {
        fetch(`/events/templates/${encodeURIComponent(id)}`, { credentials: "same-origin" })
            .then((response) => {
                if (!response.ok) throw new Error(String(response.status));
                return response.json();
            })
            .then((template) => {
                title.value = template.title;
                titleTouched = true;
                form.querySelector("#description").value = template.description;
                check("kind", template.kind);
                check("severity", template.severity);
                check("category", template.category);
                form.querySelector("[data-template-id]").value = String(template.id);
                closeSuggestions();
                update();
                title.focus();
            })
            .catch(showTemplateError);
    }

    const templateMenu = document.querySelector("[data-template-menu]");
    if (templateMenu) {
        templateMenu.addEventListener("click", (event) => {
            const choice = event.target.closest("[data-template-choice]");
            if (!choice) return;
            templateMenu.open = false;
            applyTemplate(choice.dataset.templateChoice);
        });
    }

    form.addEventListener("change", update);
    title.addEventListener("input", () => {
        titleTouched = title.value.trim() !== "";
        form.querySelector("[data-template-id]").value = "";
    });
    // A proposed title is a suggestion: typing replaces it rather than
    // extending it.
    title.addEventListener("focus", () => {
        if (!titleTouched && title.value) title.select();
    });

    if (suggestions) {
        suggestions.addEventListener("htmx:afterSwap", () => {
            title.setAttribute("aria-expanded", String(Boolean(suggestions.querySelector("[data-template]"))));
        });
        suggestions.addEventListener("click", (event) => {
            const option = event.target.closest("[data-template]");
            if (option) applyTemplate(option.dataset.template);
        });
        // Arrows move between the title and the suggestions; Escape closes them.
        form.addEventListener("keydown", (event) => {
            const options = Array.from(suggestions.querySelectorAll("[data-template]"));
            if (event.key === "Escape" && options.length) {
                closeSuggestions();
                title.focus();
                return;
            }
            if (!options.length || (event.key !== "ArrowDown" && event.key !== "ArrowUp")) return;
            const index = options.indexOf(document.activeElement);
            if (event.target !== title && index === -1) return;
            event.preventDefault();
            const step = event.key === "ArrowDown" ? 1 : -1;
            const next = index + step;
            if (next < 0) title.focus();
            else options[Math.min(next, options.length - 1)].focus();
        });
        document.addEventListener("click", (event) => {
            if (event.target instanceof Element && !event.target.closest(".suggest")) closeSuggestions();
        });
        form.addEventListener("focusout", (event) => {
            const next = event.relatedTarget;
            if (next instanceof Element && !next.closest(".suggest")) closeSuggestions();
        });
    }

    update();
})();

// The fields of a notification destination: the address row says what to
// paste for the chosen tool, with its example and where to find it. A test
// result belongs to the destination it was run for, so changing that
// destination clears it.
(function () {
    "use strict";

    document.querySelectorAll("[data-destination-form]").forEach((form) => {
        const kind = form.querySelector("[data-destination-kind]");
        const target = form.querySelector('input[name="target"]');
        const label = form.querySelector("[data-target-label]");
        if (!kind || !target) return;

        function clearResult() {
            form.querySelectorAll("[data-test-result]").forEach((result) => result.replaceChildren());
        }

        function update() {
            const option = kind.selectedOptions[0];
            if (!option) return;
            if (label) label.textContent = option.dataset.label;
            target.placeholder = option.dataset.placeholder;
            target.inputMode = option.dataset.inputmode;
            form.querySelectorAll("[data-hint-for]").forEach((hint) => {
                hint.hidden = hint.dataset.hintFor !== kind.value;
            });
            const describedBy = target.getAttribute("aria-describedby") || "";
            const others = describedBy.split(" ").filter((id) => id && !id.startsWith("hint-"));
            target.setAttribute("aria-describedby", ["hint-" + kind.value, ...others].join(" "));
        }

        form.addEventListener("change", (event) => {
            if (event.target === kind) {
                update();
                clearResult();
            }
        });
        target.addEventListener("input", clearResult);
        update();
    });
})();

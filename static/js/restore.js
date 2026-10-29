// Restoring at first launch: the file field only shows for a file from
// this computer, and says the name of the file picked.
(function () {
    "use strict";

    const form = document.querySelector("[data-restore]");
    if (!form) return;
    const field = form.querySelector("[data-restore-file]");
    const input = form.querySelector("[data-restore-input]");
    const name = form.querySelector("[data-restore-name]");
    const sent = form.querySelector("[data-restore-sent]");

    form.addEventListener("change", (event) => {
        if (event.target === input && input.files.length) {
            name.textContent = input.files[0].name;
            return;
        }
        if (sent) field.hidden = !sent.checked;
    });
})();

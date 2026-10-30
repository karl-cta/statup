// Restoring at first launch: the file field only shows for a file from
// this computer.
(function () {
    "use strict";

    const form = document.querySelector("[data-restore]");
    if (!form) return;
    const field = form.querySelector("[data-restore-file]");
    const sent = form.querySelector("[data-restore-sent]");
    if (!sent) return;

    form.addEventListener("change", (event) => {
        if (event.target.matches("input[type=file]")) return;
        field.hidden = !sent.checked;
    });
})();

// The settings page wears the colour and ambiance being chosen before they
// are saved, and the ones a refused save sent; leaving without saving
// reloads the saved ones.
(function () {
    const root = document.documentElement;
    const defaults = { accent: "ink", ambiance: "sand" };

    function wear(input) {
        if (input.value === defaults[input.name]) {
            delete root.dataset[input.name];
        } else {
            root.dataset[input.name] = input.value;
        }
        const name = document.querySelector("[data-accent-name]");
        const label = input.closest("label");
        if (input.name === "accent" && name && label) {
            name.textContent = label.title;
        }
    }

    document.addEventListener("change", (event) => {
        const input = event.target;
        if (input instanceof HTMLInputElement && input.name in defaults) {
            wear(input);
        }
    });
    document
        .querySelectorAll('input[name="accent"]:checked, input[name="ambiance"]:checked')
        .forEach(wear);
})();

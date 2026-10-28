// The settings page wears the colour and ambiance being chosen before they
// are saved; leaving without saving reloads the saved ones.
(function () {
    const root = document.documentElement;
    const defaults = { accent: "ink", ambiance: "sand" };
    document.addEventListener("change", (event) => {
        const input = event.target;
        if (!(input instanceof HTMLInputElement) || !(input.name in defaults)) {
            return;
        }
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
    });
})();

// The status embed set to follow the device: dark when the reader's system
// is, before the first paint. A frame that blocks scripts stays light.
if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
    document.documentElement.classList.add("dark");
}

// Custom select component, progressive enhancement over native <select>.
// Mark a <select> with data-custom-select to replace its rendering with an
// ARIA listbox combobox. The native element stays in the DOM for form
// submission and HTMX change triggers; we dispatch a bubbling 'change' on it
// whenever the user picks an option in the overlay.

(function () {
    var CARET_SVG =
        '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
        '<path d="M3 4.5 6 7.5 9 4.5"/></svg>';

    function initSelect(native) {
        if (native.dataset.csInitialized === '1') return;
        native.dataset.csInitialized = '1';

        var baseId = native.id || 'cs-' + Math.random().toString(36).slice(2, 9);
        if (!native.id) native.id = baseId;

        var wrap = document.createElement('div');
        wrap.className = 'cs-wrap';
        if (native.disabled) wrap.classList.add('cs-disabled');

        var trigger = document.createElement('button');
        trigger.type = 'button';
        trigger.className = 'cs-trigger';
        trigger.setAttribute('role', 'combobox');
        trigger.setAttribute('aria-haspopup', 'listbox');
        trigger.setAttribute('aria-expanded', 'false');
        trigger.setAttribute('aria-controls', baseId + '-list');
        if (native.disabled) trigger.disabled = true;

        var labelledBy = null;
        if (native.labels && native.labels.length > 0) {
            var lbl = native.labels[0];
            if (!lbl.id) lbl.id = baseId + '-label';
            labelledBy = lbl.id;
            trigger.setAttribute('aria-labelledby', labelledBy);
        } else if (native.getAttribute('aria-label')) {
            trigger.setAttribute('aria-label', native.getAttribute('aria-label'));
        }

        var labelEl = document.createElement('span');
        labelEl.className = 'cs-label';
        var caretEl = document.createElement('span');
        caretEl.className = 'cs-caret';
        caretEl.innerHTML = CARET_SVG;
        trigger.appendChild(labelEl);
        trigger.appendChild(caretEl);

        var list = document.createElement('ul');
        list.className = 'cs-list';
        list.id = baseId + '-list';
        list.setAttribute('role', 'listbox');
        list.setAttribute('tabindex', '-1');
        if (labelledBy) list.setAttribute('aria-labelledby', labelledBy);
        list.hidden = true;

        function rebuildOptions() {
            list.innerHTML = '';
            Array.prototype.forEach.call(native.options, function (opt, idx) {
                var item = document.createElement('li');
                item.className = 'cs-option';
                item.setAttribute('role', 'option');
                item.dataset.value = opt.value;
                item.id = baseId + '-opt-' + idx;
                item.textContent = opt.textContent;
                if (opt.disabled) {
                    item.setAttribute('aria-disabled', 'true');
                    item.classList.add('cs-option-disabled');
                }
                if (opt.selected) item.setAttribute('aria-selected', 'true');
                list.appendChild(item);
            });
        }
        rebuildOptions();

        native.parentNode.insertBefore(wrap, native);
        wrap.appendChild(native);
        wrap.appendChild(trigger);
        document.body.appendChild(list);
        native.classList.add('cs-native');
        native.setAttribute('tabindex', '-1');
        native.setAttribute('aria-hidden', 'true');

        var isOpen = false;
        var activeIndex = -1;
        var typeBuffer = '';
        var typeTimer = null;

        function syncLabel() {
            var opt = native.options[native.selectedIndex];
            labelEl.textContent = opt ? opt.textContent : '';
            Array.prototype.forEach.call(list.children, function (li) {
                if (li.dataset.value === native.value) li.setAttribute('aria-selected', 'true');
                else li.removeAttribute('aria-selected');
            });
        }
        syncLabel();

        function currentIndex() {
            var items = list.children;
            for (var i = 0; i < items.length; i++) {
                if (items[i].dataset.value === native.value) return i;
            }
            return 0;
        }

        function setActive(idx) {
            var items = list.children;
            if (items.length === 0) return;
            if (idx < 0) idx = 0;
            if (idx >= items.length) idx = items.length - 1;
            Array.prototype.forEach.call(items, function (li) { li.classList.remove('cs-active'); });
            var target = items[idx];
            target.classList.add('cs-active');
            activeIndex = idx;
            trigger.setAttribute('aria-activedescendant', target.id);
            var t = target.offsetTop;
            var b = t + target.offsetHeight;
            if (t < list.scrollTop) list.scrollTop = t;
            else if (b > list.scrollTop + list.clientHeight) list.scrollTop = b - list.clientHeight;
        }

        function positionList() {
            var rect = trigger.getBoundingClientRect();
            var viewportH = window.innerHeight;
            var spaceBelow = viewportH - rect.bottom;
            var spaceAbove = rect.top;
            var maxH = 280;
            var openUp = spaceBelow < Math.min(maxH, 200) && spaceAbove > spaceBelow;
            list.style.position = 'fixed';
            list.style.left = rect.left + 'px';
            list.style.minWidth = rect.width + 'px';
            if (openUp) {
                list.style.top = '';
                list.style.bottom = (viewportH - rect.top + 4) + 'px';
                list.style.maxHeight = Math.max(120, spaceAbove - 16) + 'px';
            } else {
                list.style.bottom = '';
                list.style.top = (rect.bottom + 4) + 'px';
                list.style.maxHeight = Math.max(120, spaceBelow - 16) + 'px';
            }
        }

        function open() {
            if (isOpen || native.disabled) return;
            isOpen = true;
            list.hidden = false;
            positionList();
            trigger.setAttribute('aria-expanded', 'true');
            wrap.classList.add('cs-open');
            setActive(currentIndex());
            document.addEventListener('mousedown', onOutside, true);
            window.addEventListener('resize', onViewportChange, true);
            window.addEventListener('scroll', onViewportChange, true);
        }

        function close(focusTrigger) {
            if (!isOpen) return;
            isOpen = false;
            list.hidden = true;
            trigger.setAttribute('aria-expanded', 'false');
            wrap.classList.remove('cs-open');
            Array.prototype.forEach.call(list.children, function (li) { li.classList.remove('cs-active'); });
            trigger.removeAttribute('aria-activedescendant');
            document.removeEventListener('mousedown', onOutside, true);
            window.removeEventListener('resize', onViewportChange, true);
            window.removeEventListener('scroll', onViewportChange, true);
            if (focusTrigger) trigger.focus();
        }

        function onOutside(e) {
            if (!wrap.contains(e.target) && !list.contains(e.target)) close(false);
        }

        function onViewportChange() { close(false); }

        function commit(idx) {
            var items = list.children;
            if (idx < 0 || idx >= items.length) return;
            var item = items[idx];
            if (item.getAttribute('aria-disabled') === 'true') return;
            var value = item.dataset.value;
            var changed = value !== native.value;
            native.value = value;
            syncLabel();
            if (changed) native.dispatchEvent(new Event('change', { bubbles: true }));
            close(true);
        }

        function handleType(ch) {
            if (!isOpen) open();
            clearTimeout(typeTimer);
            typeBuffer += ch.toLowerCase();
            typeTimer = setTimeout(function () { typeBuffer = ''; }, 500);
            var items = list.children;
            for (var i = 0; i < items.length; i++) {
                var probe = (i + activeIndex + 1) % items.length;
                if (items[probe].textContent.trim().toLowerCase().indexOf(typeBuffer) === 0) {
                    setActive(probe);
                    return;
                }
            }
        }

        trigger.addEventListener('click', function () {
            if (isOpen) close(true); else open();
        });

        trigger.addEventListener('keydown', function (e) {
            switch (e.key) {
                case 'ArrowDown':
                    e.preventDefault();
                    if (!isOpen) open();
                    else setActive(activeIndex + 1);
                    break;
                case 'ArrowUp':
                    e.preventDefault();
                    if (!isOpen) open();
                    else setActive(activeIndex - 1);
                    break;
                case 'Home':
                    if (isOpen) { e.preventDefault(); setActive(0); }
                    break;
                case 'End':
                    if (isOpen) { e.preventDefault(); setActive(list.children.length - 1); }
                    break;
                case 'Enter':
                case ' ':
                    e.preventDefault();
                    if (!isOpen) open();
                    else commit(activeIndex);
                    break;
                case 'Escape':
                    if (isOpen) { e.preventDefault(); close(true); }
                    break;
                case 'Tab':
                    if (isOpen) close(false);
                    break;
                default:
                    if (e.key && e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
                        e.preventDefault();
                        handleType(e.key);
                    }
            }
        });

        list.addEventListener('mousedown', function (e) {
            var li = e.target.closest('[role="option"]');
            if (!li) return;
            e.preventDefault();
            var idx = Array.prototype.indexOf.call(list.children, li);
            commit(idx);
        });

        list.addEventListener('mouseover', function (e) {
            var li = e.target.closest('[role="option"]');
            if (!li) return;
            var idx = Array.prototype.indexOf.call(list.children, li);
            if (idx >= 0) setActive(idx);
        });

        native.addEventListener('change', function (e) {
            if (e.isTrusted) syncLabel();
        });
    }

    function initAll(root) {
        var scope = root && root.querySelectorAll ? root : document;
        scope.querySelectorAll('select[data-custom-select]').forEach(initSelect);
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', function () { initAll(); });
    } else {
        initAll();
    }
    document.body.addEventListener('htmx:afterSwap', function (e) { initAll(e.target); });
})();

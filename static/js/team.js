// Guards on the team page. A role change and a disable both ask in the row,
// naming the person and what changes, before anything is posted.
(function () {
    'use strict';

    function conceal(box) {
        if (box) box.hidden = true;
    }

    // The native select sits behind a dressed listbox: putting the value back
    // has to tell that widget too, without firing a change of its own.
    function restore(select) {
        select.value = select.dataset.committed;
        select.dispatchEvent(new CustomEvent('cs:sync'));
    }

    document.addEventListener('change', function (e) {
        var select = e.target;
        if (!select.matches || !select.matches('[data-role-form] select[name="role"]')) return;
        var form = select.closest('[data-role-form]');
        var box = form.querySelector('[data-confirm]');
        if (select.value === select.dataset.committed) {
            conceal(box);
            return;
        }
        var label = select.options[select.selectedIndex].text;
        var desc = select.dataset['desc' + select.value.charAt(0).toUpperCase() + select.value.slice(1)];
        box.querySelector('[data-confirm-label]').textContent = select.dataset.name + ' \u2192 ' + label;
        box.querySelector('[data-confirm-desc]').textContent = desc;
        box.hidden = false;
        box.querySelector('.status-confirm-go').focus();
    });

    document.addEventListener('click', function (e) {
        if (!e.target.closest) return;

        var disable = e.target.closest('[data-disable-btn]');
        if (disable) {
            var box = disable.parentNode.querySelector('[data-confirm]');
            disable.hidden = true;
            box.hidden = false;
            box.querySelector('.status-confirm-cancel').focus();
            return;
        }

        var cancel = e.target.closest('[data-confirm-cancel]');
        if (!cancel) return;
        var form = cancel.closest('form');
        conceal(cancel.closest('[data-confirm]'));
        var select = form.querySelector('select[name="role"]');
        if (select) {
            restore(select);
            form.querySelector('.cs-trigger, select').focus();
            return;
        }
        var opener = form.querySelector('[data-disable-btn]');
        opener.hidden = false;
        opener.focus();
    });
})();

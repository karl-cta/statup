// Guards on the service status control.
//
// This is the one control in the product that publishes to every visitor, and
// it used to post on the first keystroke: a stray scroll over a focused select
// announced a major outage with no confirmation, no receipt and no way back.
// Two guards sit on it now. The two outage levels ask before publishing, and
// every publication comes back with a receipt carrying an undo.
(function () {
    'use strict';

    var GUARDED = ['partial_outage', 'major_outage'];
    var RECEIPT_MS = 9000;

    function reveal(host, select) {
        var box = host.querySelector('[data-status-confirm]');
        box.querySelector('[data-status-confirm-label]').textContent =
            select.options[select.selectedIndex].text;
        box.hidden = false;
        box.querySelector('[data-status-confirm-go]').focus();
    }

    function conceal(host) {
        var box = host.querySelector('[data-status-confirm]');
        if (box) box.hidden = true;
    }

    // The native select is hidden behind a custom listbox, so putting the value
    // back has to tell that widget too. `cs:sync` redraws its trigger without
    // firing a change, which would read as a fresh choice.
    function restore(select) {
        select.value = select.dataset.statusCommitted;
        select.dispatchEvent(new CustomEvent('cs:sync'));
    }

    document.addEventListener('change', function (e) {
        var select = e.target;
        if (!select.matches || !select.matches('[data-status-form] select[name="status"]')) return;

        var host = select.closest('.status-cell');
        if (!host || select.value === select.dataset.statusCommitted) return;

        if (GUARDED.indexOf(select.value) !== -1) {
            reveal(host, select);
            return;
        }
        conceal(host);
        host.querySelector('[data-status-form]').requestSubmit();
    });

    document.addEventListener('click', function (e) {
        if (!e.target.closest) return;

        var go = e.target.closest('[data-status-confirm-go]');
        if (go) {
            var publishing = go.closest('.status-cell');
            conceal(publishing);
            publishing.querySelector('[data-status-form]').requestSubmit();
            return;
        }

        var cancel = e.target.closest('[data-status-confirm-cancel]');
        if (!cancel) return;

        var host = cancel.closest('.status-cell');
        conceal(host);
        restore(host.querySelector('select[name="status"]'));
        var trigger = host.querySelector('.cs-trigger');
        if (trigger) trigger.focus();
    });

    // The value the server actually holds, which is where a cancelled choice
    // has to fall back to.
    function init(root) {
        var scope = root && root.querySelectorAll ? root : document;

        scope.querySelectorAll('[data-status-form] select[name="status"]').forEach(function (select) {
            select.dataset.statusCommitted = select.value;
        });

        scope.querySelectorAll('[data-status-receipt]').forEach(function (receipt) {
            if (receipt.dataset.armed) return;
            receipt.dataset.armed = '1';
            window.setTimeout(function () {
                receipt.remove();
            }, RECEIPT_MS);
        });
    }

    init(document);
    document.body.addEventListener('htmx:afterSwap', function () {
        init(document);
    });
})();

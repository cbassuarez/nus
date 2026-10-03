// nus dock motion: nus's launch, on its own dock icon.
//
// GNOME gives an app no way to change its dock icon while it runs; the
// dash, Ubuntu Dock and Dash to Dock all draw the icon of the app's desktop
// entry. This extension lets nus, and only nus, play a few frames there:
// nus calls Launch() on GNOME Shell's bus as it starts and Ready() when its
// window is up; every icon on screen showing nus's icon steps through the
// frames on the beat, finishes the pass, and gets its own icon back.
//
// It accepts only an app id beginning dev.nus.app, at most twelve PNGs from
// nus's own icons folder, and runs at most fifteen seconds.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const IFACE = `<node>
  <interface name="dev.nus.DockMotion">
    <method name="Launch">
      <arg type="s" name="appId" direction="in"/>
      <arg type="as" name="frames" direction="in"/>
      <arg type="u" name="stepMs" direction="in"/>
    </method>
    <method name="Ready">
      <arg type="s" name="appId" direction="in"/>
    </method>
    <property name="Version" type="u" access="read"/>
  </interface>
</node>`;

const LONGEST_US = 15 * 1000 * 1000;

export default class DockMotion extends Extension {
    enable() {
        this._runs = new Map();
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._dbus.export(Gio.DBus.session, '/dev/nus/DockMotion');
    }

    disable() {
        for (const id of [...this._runs.keys()])
            this._stop(id);
        this._dbus?.unexport();
        this._dbus = null;
        this._runs = null;
    }

    get Version() {
        return 1;
    }

    Launch(appId, frames, stepMs) {
        if (!this._runs || !/^dev\.nus\.app(\.[a-z]+)?$/.test(appId))
            return;
        const folder = `${GLib.get_user_data_dir()}/nus/icons/`;
        if (frames.length === 0 || frames.length > 12 ||
            !frames.every(f => f.startsWith(folder) && f.endsWith('.png') && !f.includes('..')))
            return;
        this._stop(appId);
        const app = Shell.AppSystem.get_default().lookup_app(`${appId}.desktop`);
        if (!app)
            return;
        const run = {
            app,
            gicons: frames.map(f => Gio.FileIcon.new(Gio.File.new_for_path(f))),
            index: 0,
            ready: false,
            swapped: new Map(),
            started: GLib.get_monotonic_time(),
            timer: 0,
        };
        const step = Math.max(40, Math.min(1000, stepMs));
        run.timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, step, () => this._tick(appId));
        this._runs.set(appId, run);
        this._paint(run);
    }

    Ready(appId) {
        const run = this._runs?.get(appId);
        if (run)
            run.ready = true;
    }

    _tick(appId) {
        const run = this._runs?.get(appId);
        if (!run)
            return GLib.SOURCE_REMOVE;
        const late = GLib.get_monotonic_time() - run.started > LONGEST_US;
        if (late || (run.ready && run.index === run.gicons.length - 1)) {
            run.timer = 0;
            this._stop(appId);
            return GLib.SOURCE_REMOVE;
        }
        run.index = (run.index + 1) % run.gicons.length;
        this._paint(run);
        return GLib.SOURCE_CONTINUE;
    }

    // Every icon on screen drawing this app's icon, whichever dock drew it;
    // looked for each frame, since a dock makes new ones as the app starts.
    _icons(run) {
        const original = run.app.get_icon();
        const out = [];
        const walk = actor => {
            if (actor instanceof St.Icon && actor.gicon &&
                (run.swapped.has(actor) || actor.gicon.equal(original)))
                out.push(actor);
            for (const child of actor.get_children())
                walk(child);
        };
        walk(Main.layoutManager.uiGroup);
        return out;
    }

    _paint(run) {
        for (const icon of this._icons(run)) {
            if (!run.swapped.has(icon)) {
                run.swapped.set(icon, icon.gicon);
                icon.connectObject('destroy', () => run.swapped.delete(icon), this);
            }
            icon.gicon = run.gicons[run.index];
        }
    }

    _stop(appId) {
        const run = this._runs?.get(appId);
        if (!run)
            return;
        if (run.timer)
            GLib.source_remove(run.timer);
        for (const [icon, gicon] of run.swapped) {
            icon.disconnectObject(this);
            icon.gicon = gicon;
        }
        this._runs.delete(appId);
    }
}

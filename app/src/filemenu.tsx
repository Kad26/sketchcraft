import { useLocale } from "./locale";
// The File menu in the top bar: files, exports, plugins, and the dialogs the rest of the app opens.
import { useState, type RefObject } from "react";
import { inTauri } from "./engine";
import { keyText } from "./keymapstore";
import { loadPlugins, type Plugin, type PluginList } from "./plugins";
import { checkForUpdatesNow } from "./updater";
import { useEscapeToClose } from "./sheet";
import type { useFileActions } from "./usefiles";

export function FileMenu({ file, section, plugins, livePlugins, setPlugins, runPluginCommand, reloadPlugins, fileRef, open }: {
  file: ReturnType<typeof useFileActions>["file"];
  /** a section cut is showing (it can be exported) */
  section: boolean;
  plugins: PluginList;
  livePlugins: Plugin[];
  setPlugins: (p: PluginList) => void;
  runPluginCommand: (plugin: Plugin, commandId: string) => Promise<void> | undefined;
  reloadPlugins: () => void;
  fileRef: RefObject<HTMLInputElement | null>;
  /** the dialogs the menu opens, by what they are */
  open: { restore: () => void; plugins: () => void; sheets: () => void; layout: () => void; timeline: () => void; location: () => void; text3d: () => void; keys: () => void; keyEditor: () => void };
}) {
  const { t, preference, setPreference } = useLocale();
  const [fileMenu, setFileMenu] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [pluginsOpen, setPluginsOpen] = useState(false);
  useEscapeToClose(() => setFileMenu(false), fileMenu);
  return (
  <div className="dropdown">
    <button className="file-btn" onClick={() => setFileMenu((o) => !o)}>
      {t("menu.file")} ▾
    </button>
    {fileMenu && (
      <>
        <div className="menu-scrim" onPointerDown={() => setFileMenu(false)} />
        <div className="dropdown-menu file-menu" onClick={() => setFileMenu(false)}>
          <button onClick={file.new}>{t("menu.new")} <kbd>⌘N</kbd></button>
          <button onClick={file.open}>{t("menu.open")} <kbd>⌘O</kbd></button>
          <button onClick={() => file.save(false)}>{t("menu.save")} <kbd>⌘S</kbd></button>
          <button onClick={() => file.save(true)}>{t("menu.saveAs")} <kbd>⇧⌘S</kbd></button>
          <button onClick={() => open.restore()}>{t("menu.restore")}</button>
          <hr />
          <button onClick={file.import}>{t("menu.import")} <kbd>⌘I</kbd></button>
          <button onClick={() => fileRef.current?.click()}>{t("menu.upload")}</button>
          <button
            className="submenu-btn"
            aria-expanded={exportOpen}
            onClick={(e) => {
              e.stopPropagation();
              setExportOpen((o) => !o);
            }}
          >
            {t("menu.export")} <span className="caret">{exportOpen ? "▾" : "▸"}</span>
          </button>
          {exportOpen && (
            <div className="submenu">
              <button onClick={() => file.export("glb")}>glTF (.glb)…</button>
              <button onClick={() => file.export("obj")}>OBJ…</button>
              <button onClick={() => file.export("stl")}>STL (mm)…</button>
              <button onClick={() => file.export("dae")}>Collada (.dae)…</button>
              <button onClick={() => file.export("dxf")}>3D DXF (mm)…</button>
              <button onClick={() => file.export("dwg")}>DWG (AutoCAD)…</button>
              <button onClick={() => file.export("ifc")}>IFC (BIM)…</button>
              <button onClick={() => file.export("pdf")}>{t("menu.pdf")}</button>
              <button onClick={file.render}>{t("menu.png")}</button>
              <button onClick={file.animation}>{t("menu.tour")}</button>
              {section && (
                <>
                  <button onClick={() => file.section("svg")}>{t("menu.svgSection")}</button>
                  <button onClick={() => file.section("dxf")}>{t("menu.dxfSection")}</button>
                </>
              )}
            </div>
          )}
          <button
            className="submenu-btn"
            aria-expanded={pluginsOpen}
            onClick={(e) => {
              e.stopPropagation();
              if (!pluginsOpen) loadPlugins().then(setPlugins, () => {});
              setPluginsOpen((o) => !o);
            }}
          >
            {t("menu.plugins")} <span className="caret">{pluginsOpen ? "▾" : "▸"}</span>
          </button>
          {pluginsOpen && (
            <div className="submenu">
              {livePlugins.flatMap((pl) =>
                pl.commands.map((c) => (
                  <button key={`${pl.id}:${c.id}`} title={c.hint ?? pl.description} onClick={() => void runPluginCommand(pl, c.id)}>
                    {c.label}
                  </button>
                )),
              )}
              {plugins.problems.map((p) => (
                <button key={`x${p.id}`} disabled title={p.error}>
                  {p.id}: {p.error}
                </button>
              ))}
              <button onClick={() => (reloadPlugins(), open.plugins())}>{t("menu.manage")}</button>
            </div>
          )}
          <hr />
          <button onClick={file.blueprint}>{t("menu.blueprint")}</button>
          <button onClick={() => open.layout()} title={t("menu.layoutTip")}>
            {t("menu.layout")}
          </button>
          <button onClick={() => open.sheets()}>{t("menu.sheets")}</button>
          <button onClick={() => open.timeline()}>{t("menu.timeline")}</button>
          <hr />
          <button onClick={() => open.location()}>{t("menu.location")}</button>
          <button onClick={() => open.text3d()}>{t("menu.text3d")}</button>
          <button onClick={file.report}>{t("menu.report")}</button>
          <hr />
          <button onClick={() => open.keys()}>
            {t("menu.shortcuts")} <kbd>{keyText("help.shortcuts")}</kbd>
          </button>
          <button onClick={() => open.keyEditor()}>{t("menu.customize")}</button>
          {inTauri && <button onClick={checkForUpdatesNow}>{t("menu.updates")}</button>}
          <label className="language-preference" onClick={(event) => event.stopPropagation()}>
            {t("language.label")}{" "}
            <select value={preference} onChange={(event) => setPreference(event.currentTarget.value)}>
              <option value="system">{t("language.system")}</option>
              <option value="en" lang="en">English</option>
              <option value="it" lang="it">Italiano</option>
            </select>
          </label>
          <div className="menu-version" title="The version of SketchCraft you're running">
            SketchCraft {__APP_VERSION__}
          </div>
        </div>
      </>
    )}
  </div>
  );
}

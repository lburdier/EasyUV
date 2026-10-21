"""Prépare les assets d'une release EasyUV dans dist/.

    python tools/package.py [--tag v0.2.0]

Assets produits (noms stables : l'application et Blender les retrouvent d'une release à l'autre) :
  EasyUV-windows-x64.exe (+ .sha256)   exécutable auto-mis à jour par l'application
  EasyUV-windows-x64.zip               application + easyuv-cli + README + LICENSE
  easyuv-blender-<version>.zip         extension Blender (contient easyuv.dll)
  index.json                           dépôt d'extensions Blender (mises à jour de l'addon)
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
import tomllib
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIST = os.path.join(ROOT, "dist")
REL = os.path.join(ROOT, "target", "release")
ADDON = os.path.join(ROOT, "blender_addon", "easyuv")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def versions():
    with open(os.path.join(ROOT, "Cargo.toml"), "rb") as f:
        cargo = tomllib.load(f)["workspace"]["package"]["version"]
    with open(os.path.join(ADDON, "blender_manifest.toml"), "rb") as f:
        manifest = tomllib.load(f)
    with open(os.path.join(ADDON, "__init__.py"), encoding="utf-8") as f:
        m = re.search(r'"version":\s*\((\d+),\s*(\d+),\s*(\d+)\)', f.read())
    bl_info = ".".join(m.groups()) if m else None
    return cargo, manifest, bl_info


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", help="tag git (ex. v0.2.0) : doit correspondre à la version")
    args = ap.parse_args()

    cargo, manifest, bl_info = versions()
    errors = []
    if manifest["version"] != cargo:
        errors.append("blender_manifest.toml version %s != Cargo %s" % (manifest["version"], cargo))
    if bl_info != cargo:
        errors.append("bl_info version %s != Cargo %s" % (bl_info, cargo))
    if args.tag and args.tag.lstrip("v") != cargo:
        errors.append("tag %s != version %s" % (args.tag, cargo))
    if errors:
        sys.exit("Versions incohérentes :\n  " + "\n  ".join(errors))

    if os.path.isdir(DIST):
        shutil.rmtree(DIST)
    os.makedirs(DIST)

    # 1. exécutable auto-mis à jour
    exe = os.path.join(DIST, "EasyUV-windows-x64.exe")
    shutil.copy2(os.path.join(REL, "EasyUV.exe"), exe)
    with open(exe + ".sha256", "w", newline="\n") as f:
        f.write("%s  EasyUV-windows-x64.exe\n" % sha256(exe))

    # 2. archive application
    with zipfile.ZipFile(os.path.join(DIST, "EasyUV-windows-x64.zip"), "w", zipfile.ZIP_DEFLATED) as z:
        z.write(exe, "EasyUV/EasyUV.exe")
        z.write(os.path.join(REL, "easyuv-cli.exe"), "EasyUV/easyuv-cli.exe")
        for doc in ("README.md", "LICENSE"):
            z.write(os.path.join(ROOT, doc), "EasyUV/" + doc)

    # 3. extension Blender (fichiers à la racine de l'archive)
    shutil.copy2(os.path.join(REL, "easyuv.dll"), os.path.join(ADDON, "easyuv.dll"))
    addon_zip_name = "easyuv-blender-%s.zip" % cargo
    addon_zip = os.path.join(DIST, addon_zip_name)
    with zipfile.ZipFile(addon_zip, "w", zipfile.ZIP_DEFLATED) as z:
        for name in sorted(os.listdir(ADDON)):
            p = os.path.join(ADDON, name)
            if os.path.isfile(p) and not name.endswith(".pyc"):
                z.write(p, name)
        z.write(os.path.join(ROOT, "LICENSE"), "LICENSE")

    # 4. index du dépôt d'extensions (format « v1 » de Blender). archive_url est relative :
    #    l'index et l'archive sont publiés dans la même release (…/releases/latest/download/).
    entry = {k: manifest[k] for k in (
        "schema_version", "id", "name", "version", "tagline", "maintainer", "type",
        "blender_version_min", "license") if k in manifest}
    for k in ("platforms", "tags", "website", "permissions", "blender_version_max"):
        if k in manifest:
            entry[k] = manifest[k]
    entry["archive_url"] = "./" + addon_zip_name
    entry["archive_size"] = os.path.getsize(addon_zip)
    entry["archive_hash"] = "sha256:" + sha256(addon_zip)
    with open(os.path.join(DIST, "index.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump({"version": "v1", "blocklist": [], "data": [entry]}, f, indent=2, ensure_ascii=False)

    # 5. notes de release : section du CHANGELOG pour cette version
    notes = ""
    with open(os.path.join(ROOT, "CHANGELOG.md"), encoding="utf-8") as f:
        m = re.search(r"^## \[%s\][^\n]*\n(.*?)(?=^## \[|\Z)" % re.escape(cargo), f.read(), re.S | re.M)
    if m:
        notes = m.group(1).strip()
    elif args.tag:
        sys.exit("CHANGELOG.md : section [%s] manquante" % cargo)
    notes += (
        "\n\n### Installation\n"
        "- **Application** : `EasyUV-windows-x64.zip` (ou l'exécutable seul). Les versions suivantes "
        "s'installent depuis l'application (Aide > Vérifier les mises à jour).\n"
        "- **Blender 4.2+** : Préférences > Get Extensions > Repositories > `+` > Add Remote Repository : "
        "`https://github.com/lburdier/EasyUV/releases/latest/download/index.json` "
        "(mises à jour automatiques), ou Install from Disk avec `%s`.\n" % addon_zip_name
    )
    with open(os.path.join(ROOT, "target", "release_notes.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write(notes)

    for n in sorted(os.listdir(DIST)):
        print("  dist/%-34s %10d octets" % (n, os.path.getsize(os.path.join(DIST, n))))


if __name__ == "__main__":
    main()

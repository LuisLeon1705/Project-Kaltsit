import os, subprocess, shutil, sys, zipfile

home = os.path.expanduser('~')
javac = os.environ.get('JAVAC', os.path.join(home, r'AppData\Local\Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime\java-runtime-epsilon\windows-x64\java-runtime-epsilon\bin\javac.exe'))
mc_jar = os.environ.get('MC_JAR', os.path.join(home, r'curseforge\minecraft\Install\versions\26.2\26.2.jar'))

candidates = [mc_jar]
libs_root = os.environ.get('MC_LIBS', os.path.join(home, r'curseforge\minecraft\Install\libraries'))
for root, _, files in os.walk(libs_root):
    for f in files:
        if f.endswith('.jar'):
            candidates.append(os.path.join(root, f))

# Extract Fabric API sub-jars
fapi_jar = os.environ.get('FAPI_JAR', os.path.join(home, r'curseforge\minecraft\Instances\AI\mods\fabric-api-0.161.0+26.2.jar'))
os.makedirs('temp_fapi', exist_ok=True)
with zipfile.ZipFile(fapi_jar, 'r') as z:
    for name in z.namelist():
        if name.startswith('META-INF/jars/') and name.endswith('.jar'):
            z.extract(name, 'temp_fapi')

for root, _, files in os.walk('temp_fapi'):
    for f in files:
        if f.endswith('.jar'):
            candidates.append(os.path.join(root, f))

# Filter valid jars
valid_jars = []
for p in candidates:
    try:
        with zipfile.ZipFile(p, 'r') as z:
            valid_jars.append(p)
    except Exception:
        pass

classpath = ';'.join(valid_jars)

src_dir = 'src/main/java/ai/kaltsit/companion'
out_dir = 'build/classes'
os.makedirs(out_dir, exist_ok=True)

files = sorted(os.path.join(src_dir, f) for f in os.listdir(src_dir) if f.endswith('.java'))

print(f"[Build] Compiling Kaltsit Standalone Companion with {len(valid_jars)} classpath jars...")
cmd = [javac, '-cp', classpath, '-d', out_dir] + files
res = subprocess.run(cmd, capture_output=True, text=True)
print("Javac returncode:", res.returncode)
if res.stdout:
    print("STDOUT:", res.stdout)
if res.stderr:
    print("STDERR:", res.stderr)

assert res.returncode == 0, "Compilation failed!"

import json
version = json.load(open('src/main/resources/fabric.mod.json', encoding='utf-8'))['version']
jar_path = f'kaltsit-companion-standalone-{version}.jar'
mods_dir = os.environ.get('MC_MODS_DIR', os.path.join(home, r'curseforge\minecraft\Instances\AI\mods'))
target_jar = os.path.join(mods_dir, jar_path)
deploy = '--no-deploy' not in sys.argv

print("[Build] Packaging standalone jar with resources and skin...")
with zipfile.ZipFile(jar_path, 'w', compression=zipfile.ZIP_DEFLATED) as z:
    # Package all resources
    res_root = 'src/main/resources'
    for root, _, r_files in os.walk(res_root):
        for rf in r_files:
            full_p = os.path.join(root, rf)
            rel_p = os.path.relpath(full_p, res_root).replace('\\', '/')
            z.write(full_p, rel_p)

    # Package compiled classes
    for root, _, class_files in os.walk(out_dir):
        for cf in class_files:
            full_path = os.path.join(root, cf)
            rel_path = os.path.relpath(full_path, out_dir).replace('\\', '/')
            z.write(full_path, rel_path)

# The repository keeps only the jar of the current version (the app's «Accesorios» downloads it).
for old in os.listdir('.'):
    if old.startswith('kaltsit-companion-standalone-') and old.endswith('.jar') and old != jar_path:
        os.remove(old)

# The catalog points at this jar: name, version and checksum (the app verifies it).
import hashlib
catalog_path = os.path.join('..', 'catalog.json')
catalog = json.load(open(catalog_path, encoding='utf-8'))
for module in catalog['modules']:
    if module['id'] == 'minecraft-companion':
        module['version'] = version
        module['jar'] = jar_path
        module['sha256'] = hashlib.sha256(open(jar_path, 'rb').read()).hexdigest()
with open(catalog_path, 'w', encoding='utf-8') as f:
    json.dump(catalog, f, indent=2, ensure_ascii=False)
    f.write('\n')

if deploy:
    # Two versions of the same mod in mods/ stop the game from starting: old ones go first.
    for old in os.listdir(mods_dir):
        if old.startswith('kaltsit-companion-standalone-') and old != jar_path:
            os.remove(os.path.join(mods_dir, old))
    shutil.copy(jar_path, target_jar)
    print(f"[Build] Successfully built and deployed to: {target_jar}")
else:
    print(f"[Build] Built {jar_path} (not deployed)")

# Clean temp
shutil.rmtree('temp_fapi', ignore_errors=True)
shutil.rmtree('build', ignore_errors=True)

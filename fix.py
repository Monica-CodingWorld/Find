content = open('backend/src/find_api/core/storage_local.py', 'r').read()

old1 = '            full_path.parent.mkdir(parents=True, exist_ok=True)\n            with open(full_path, "wb") as f:\n                f.write(file_data)'
new1 = '            full_path.parent.mkdir(parents=True, exist_ok=True)\n            def _write():\n                with open(full_path, "wb") as f:\n                    f.write(file_data)\n            await asyncio.to_thread(_write)'

old2 = '            with open(full_path, "rb") as f:\n                data = f.read()\n            logger.info(f"Downloaded file from local storage: {object_name}")\n            return data'
new2 = '            def _read():\n                with open(full_path, "rb") as f:\n                    return f.read()\n            data = await asyncio.to_thread(_read)\n            logger.info(f"Downloaded file from local storage: {object_name}")\n            return data'

content = content.replace(old1, new1).replace(old2, new2)
open('backend/src/find_api/core/storage_local.py', 'w').write(content)
print('Done' if old1 in open('backend/src/find_api/core/storage_local.py').read() == False else 'Replaced!')

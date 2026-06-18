f = open("backend/src/find_api/core/storage_minio.py", "r")
c = f.read()
f.close()
old = "            if not self.client.bucket_exists(self.bucket):\n                self.client.make_bucket(self.bucket)"
new = "            exists = await asyncio.to_thread(self.client.bucket_exists, self.bucket)\n            if not exists:\n                await asyncio.to_thread(self.client.make_bucket, self.bucket)"
c = c.replace(old, new)
f = open("backend/src/find_api/core/storage_minio.py", "w")
f.write(c)
f.close()
print("to_thread count:", c.count("to_thread"))

f = open("frontend/src-tauri/src/lib.rs", "r")
c = f.read()
f.close()

# Fix 1: Add shutting_down flag to BackendState
old1 = """struct BackendState {
    running: bool,
    child: Option<CommandChild>,
}"""
new1 = """struct BackendState {
    running: bool,
    shutting_down: bool,
    child: Option<CommandChild>,
}"""
c = c.replace(old1, new1)

# Fix 2: Set shutting_down on exit
old2 = """                if let Some(child) = s.child.take() {
                    let _ = child.kill();
                    log::info!("Backend sidecar killed.");
                }
                s.running = false;"""
new2 = """                s.shutting_down = true;
                if let Some(child) = s.child.take() {
                    if let Err(e) = child.kill() {
                        log::error!("Failed to kill backend sidecar: {}", e);
                    } else {
                        log::info!("Backend sidecar killed.");
                    }
                }
                s.running = false;"""
c = c.replace(old2, new2)

# Fix 3: Check shutting_down before retry
old3 = """            Err(e) => {
                state.lock().unwrap().running = false;
                retry_count += 1;
                log::error!("Backend crashed: {}. Retry {}/{}", e, retry_count, MAX_RETRIES);
                if retry_count >= MAX_RETRIES {
                    log::error!("Backend failed {} times - giving up.", MAX_RETRIES);
                    let _ = app.emit("backend-failed", "Backend crashed too many times");
                    break;
                }
                tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECS)).await;"""
new3 = """            Err(e) => {
                state.lock().unwrap().running = false;
                if state.lock().unwrap().shutting_down {
                    log::info!("App is shutting down - stopping supervisor.");
                    break;
                }
                retry_count += 1;
                log::error!("Backend crashed: {}. Retry {}/{}", e, retry_count, MAX_RETRIES);
                if retry_count >= MAX_RETRIES {
                    log::error!("Backend failed {} times - giving up.", MAX_RETRIES);
                    let _ = app.emit("backend-failed", "Backend crashed too many times");
                    break;
                }
                tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECS)).await;"""
c = c.replace(old3, new3)

# Fix 4: Health check timeout + payload validation
old4 = """        match reqwest::get(HEALTH_URL).await {
            Ok(resp) if resp.status().is_success() => {
                log::info!("Backend health check passed on attempt {}.", attempt);
                return Ok(());
            }"""
new4 = """        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
        match client.get(HEALTH_URL).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.text().await {
                    if body.contains("healthy") {
                        log::info!("Backend health check passed on attempt {}.", attempt);
                        return Ok(());
                    }
                }
            }"""
c = c.replace(old4, new4)

# Fix 5: Kill child on health check failure
old5 = """        Err(e) => {
            log::error!("Health check failed: {}", e);
            let _ = app.emit("backend-failed", e.clone());
            return Err(e);
        }"""
new5 = """        Err(e) => {
            log::error!("Health check failed: {}", e);
            let mut s = state.lock().unwrap();
            if let Some(child) = s.child.take() {
                let _ = child.kill();
                log::info!("Killed unhealthy backend sidecar.");
            }
            s.running = false;
            return Err(e);
        }"""
c = c.replace(old5, new5)

f = open("frontend/src-tauri/src/lib.rs", "w")
f.write(c)
f.close()
print("Done! shutting_down count:", c.count("shutting_down"), "timeout count:", c.count("timeout"))

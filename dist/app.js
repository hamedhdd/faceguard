// FaceGuard Frontend Controller

const getInvoke = () => {
  if (window.__TAURI__?.core?.invoke) return window.__TAURI__.core.invoke;
  if (window.__TAURI__?.invoke) return window.__TAURI__.invoke;
  return null;
};

const invoke = async (cmd, args = {}) => {
  const tauriInvoke = getInvoke();
  if (tauriInvoke) {
    return await tauriInvoke(cmd, args);
  }
  console.warn('Tauri IPC not detected, running in preview mode:', cmd, args);
  if (cmd === 'get_telemetry') {
    return {
      state: 'monitoring',
      enabled: true,
      camera_index: 0,
      camera_name: 'HP HD Camera',
      consecutive_failed_checks: 0,
      threshold: 2,
      interval_seconds: 20,
      confidence_threshold: 0.70,
      last_confidence: 0.99,
      last_check_time: new Date().toISOString(),
      last_detection_time: new Date().toISOString(),
      next_check_time: new Date(Date.now() + 20000).toISOString(),
      last_error: null,
      total_checks: 12,
      total_locks: 0,
      recognition_enabled: false,
      recognition_threshold: 0.60,
      is_enrolled: false,
      last_similarity: 0.0,
      last_recognized: null
    };
  }
  if (cmd === 'get_config') {
    return {
      enabled: true,
      detection_interval_seconds: 20,
      missing_face_threshold: 2,
      confidence_threshold: 0.70,
      minimum_face_size: 60,
      camera_index: 0,
      start_with_windows: false,
      minimize_to_tray: true,
      notifications_enabled: true,
      lock_on_missing_face: true,
      grace_period_seconds: 0,
      recognition_enabled: false,
      recognition_threshold: 0.60,
      enrolled_face: null
    };
  }
  if (cmd === 'list_cameras') {
    return [{ index: 0, name: 'HP HD Camera', is_default: true }];
  }
  if (cmd === 'enroll_face') {
    return {
      success: false,
      confidence_percent: 0.0,
      message: 'Connecting to camera hardware for enrollment...'
    };
  }
  return {};
};

// State cache
let lastState = null;
let currentConfig = null;

// Tab management
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.tab-pane').forEach(p => p.classList.remove('active'));
    
    btn.classList.add('active');
    const tabName = btn.getAttribute('data-tab');
    document.getElementById(`pane-${tabName}`).classList.add('active');

    if (tabName === 'settings') {
      loadSettings();
    }
  });
});

function formatTime(isoStr) {
  if (!isoStr) return '--:--:--';
  const d = new Date(isoStr);
  return d.toTimeString().split(' ')[0];
}

function addLogEntry(message) {
  const log = document.getElementById('activityLog');
  const item = document.createElement('div');
  item.className = 'log-item';
  const time = new Date().toTimeString().split(' ')[0];
  item.innerHTML = `
    <span class="log-item-msg">${message}</span>
    <span class="log-item-time">${time}</span>
  `;
  log.prepend(item);
  while (log.children.length > 25) {
    log.removeChild(log.lastChild);
  }
}

// Telemetry Polling Loop
async function pollTelemetry() {
  try {
    const data = await invoke('get_telemetry');
    updateDashboard(data);
  } catch (err) {
    console.error('Error fetching telemetry:', err);
  }
}

function updateDashboard(t) {
  const statusPill = document.getElementById('statusPill');
  const statusText = document.getElementById('statusText');
  const bannerTitle = document.getElementById('bannerTitle');
  const bannerSubtitle = document.getElementById('bannerSubtitle');
  const bannerToggleBtn = document.getElementById('bannerToggleBtn');
  const bannerToggleText = document.getElementById('bannerToggleText');

  // Format state name
  const stateLabels = {
    'initializing': 'Initializing',
    'monitoring': 'Monitoring',
    'face_detected': 'Face detected',
    'face_missing': 'Face missing',
    'lock_requested': 'Lock requested',
    'locked_standby': 'Locked standby',
    'camera_error': 'Camera error',
    'disabled': 'Disabled',
    'shutting_down': 'Shutting down'
  };

  const label = stateLabels[t.state] || t.state;
  statusText.textContent = label;
  statusPill.className = `status-pill ${t.state}`;

  // Log state transition
  if (lastState && lastState !== t.state) {
    addLogEntry(`State transition: ${stateLabels[lastState] || lastState} → ${label}`);
  }
  lastState = t.state;

  // Banner status
  if (t.state === 'disabled') {
    bannerTitle.textContent = 'Monitoring Disabled';
    bannerSubtitle.textContent = 'Face detection is suspended by kill switch. Webcam hardware is released.';
    bannerToggleBtn.className = 'btn btn-emerald';
    bannerToggleText.textContent = 'Resume monitoring';
  } else if (t.state === 'locked_standby') {
    bannerTitle.textContent = 'Workstation Locked Standby';
    bannerSubtitle.textContent = 'Camera capture suspended while Windows is locked. Awaiting session unlock.';
    bannerToggleBtn.className = 'btn btn-secondary';
    bannerToggleText.textContent = 'Locked standby';
  } else if (t.state === 'camera_error') {
    bannerTitle.textContent = 'Camera Inaccessible';
    bannerSubtitle.textContent = t.last_error || 'Camera error encountered. Will retry automatically next interval.';
    bannerToggleBtn.className = 'btn btn-secondary';
    bannerToggleText.textContent = 'Check camera';
  } else {
    bannerTitle.textContent = 'Face Presence Monitoring Active';
    bannerSubtitle.textContent = `Monitoring on ${t.camera_name}. Check interval: ${t.interval_seconds}s.`;
    bannerToggleBtn.className = 'btn btn-danger';
    bannerToggleText.textContent = 'Disable (kill switch)';
  }

  // Key metrics
  document.getElementById('metricAbsence').textContent = `${t.consecutive_failed_checks} / ${t.threshold}`;
  document.getElementById('metricInterval').textContent = `${t.interval_seconds}s`;
  document.getElementById('metricLastDetect').textContent = formatTime(t.last_detection_time);
  document.getElementById('metricLastConfidence').textContent = `Confidence: ${(t.last_confidence * 100).toFixed(1)}%`;
  document.getElementById('metricNextCheck').textContent = formatTime(t.next_check_time);
  document.getElementById('metricTotalChecks').textContent = `Total checks: ${t.total_checks}`;
  document.getElementById('deviceCameraName').textContent = t.camera_name || `Camera #${t.camera_index}`;

  // Face recognition metrics
  const recMode = document.getElementById('metricRecMode');
  const recEnrolled = document.getElementById('metricRecEnrolled');
  const recScore = document.getElementById('metricRecScore');
  const recStatus = document.getElementById('metricRecStatus');

  if (recMode) {
    if (t.recognition_enabled) {
      recMode.textContent = 'Owner Recognition Active';
      recMode.style.color = 'var(--accent-emerald)';
    } else {
      recMode.textContent = 'Presence Only Mode';
      recMode.style.color = 'var(--text-primary)';
    }
  }

  if (recEnrolled) {
    if (t.is_enrolled) {
      recEnrolled.textContent = 'Owner Profile: Enrolled (512-D MobileFaceNet)';
      recEnrolled.style.color = 'var(--accent-emerald)';
    } else {
      recEnrolled.textContent = 'Owner Profile: Not Enrolled';
      recEnrolled.style.color = 'var(--text-muted)';
    }
  }

  if (recScore && recStatus) {
    if (t.last_similarity > 0) {
      const simPct = (t.last_similarity * 100).toFixed(1);
      const threshPct = (t.recognition_threshold * 100).toFixed(0);
      const authorized = t.last_recognized === true;
      recScore.textContent = `${simPct}% (${authorized ? 'Authorized' : 'Unauthorized'})`;
      recScore.style.color = authorized ? 'var(--accent-emerald)' : 'var(--accent-rose)';
      recStatus.textContent = `Threshold: ${threshPct}% match required`;
    } else {
      recScore.textContent = '--%';
      recScore.style.color = 'var(--text-primary)';
      recStatus.textContent = `Threshold: ${(t.recognition_threshold * 100).toFixed(0)}% match required`;
    }
  }
}

// Controls & Actions
document.getElementById('headerKillSwitch').addEventListener('click', async () => {
  await invoke('disable_monitoring');
  addLogEntry('Kill switch triggered from header');
  pollTelemetry();
});

document.getElementById('headerLockNow').addEventListener('click', async () => {
  try {
    await invoke('lock_windows_now');
    addLogEntry('Workstation lock executed');
    pollTelemetry();
  } catch (e) {
    alert(`Failed to lock workstation: ${e}`);
  }
});

document.getElementById('btnEnableMon').addEventListener('click', async () => {
  await invoke('enable_monitoring');
  addLogEntry('Monitoring manually enabled');
  pollTelemetry();
});

document.getElementById('btnDisableMon').addEventListener('click', async () => {
  await invoke('disable_monitoring');
  addLogEntry('Monitoring manually disabled');
  pollTelemetry();
});

document.getElementById('btnLockNow').addEventListener('click', async () => {
  try {
    await invoke('lock_windows_now');
    addLogEntry('Workstation lock executed');
    pollTelemetry();
  } catch (e) {
    alert(`Failed to lock workstation: ${e}`);
  }
});

document.getElementById('bannerToggleBtn').addEventListener('click', async () => {
  const t = await invoke('get_telemetry');
  if (t.state === 'disabled') {
    await invoke('resume_monitoring');
    addLogEntry('Monitoring resumed');
  } else {
    await invoke('disable_monitoring');
    addLogEntry('Monitoring disabled');
  }
  pollTelemetry();
});

// Diagnostics
document.getElementById('btnTestCamera').addEventListener('click', async () => {
  const out = document.getElementById('diagOutput');
  out.textContent = 'Accessing camera device for single-frame test...';
  try {
    const res = await invoke('test_camera');
    if (res.success) {
      out.textContent = `[OK] Camera captured successfully!\n` +
        `Device: ${res.camera_name}\n` +
        `Resolution: ${res.width}x${res.height}\n` +
        `Details: ${res.message}`;
      addLogEntry(`Camera diagnostic passed: ${res.width}x${res.height}`);
    } else {
      out.textContent = `[ERROR] Camera test failed!\nDevice: ${res.camera_name}\n${res.message}`;
      addLogEntry(`Camera diagnostic failed: ${res.message}`);
    }
  } catch (err) {
    out.textContent = `[ERROR] Exception testing camera: ${err}`;
  }
});

document.getElementById('btnTestDetection').addEventListener('click', async () => {
  const out = document.getElementById('diagOutput');
  out.textContent = 'Capturing frame and running ONNX face presence inference...';
  try {
    const res = await invoke('test_detection');
    if (res.success) {
      out.textContent = `[OK] Face Presence Check Completed!\n` +
        `Presence detected: ${res.face_detected ? 'YES (Human face detected)' : 'NO (Face absent)'}\n` +
        `Confidence: ${res.confidence_percent.toFixed(1)}%\n` +
        `Inference duration: ${res.inference_ms.toFixed(1)} ms\n` +
        `Result: ${res.message}`;
      addLogEntry(`Detection diagnostic: present=${res.face_detected}, conf=${res.confidence_percent.toFixed(1)}%`);
    } else {
      out.textContent = `[ERROR] Detection test failed!\n${res.message}`;
      addLogEntry(`Detection diagnostic error: ${res.message}`);
    }
  } catch (err) {
    out.textContent = `[ERROR] Exception during detection test: ${err}`;
  }
});

// Face Recognition Operations
document.getElementById('btnEnrollFace').addEventListener('click', async () => {
  const out = document.getElementById('recOutput');
  out.textContent = 'Capturing frame and enrolling your face profile... Please look directly at the webcam.';
  try {
    const res = await invoke('enroll_face');
    console.log('enroll_face response:', res);
    if (res && res.success) {
      const conf = res.confidence_percent !== undefined ? res.confidence_percent : (res.confidencePercent !== undefined ? res.confidencePercent : 0.0);
      const msg = res.message || 'Owner face enrolled successfully!';
      out.textContent = `[SUCCESS] ${msg}\n` +
        `Face Confidence: ${Number(conf).toFixed(1)}%\n` +
        `FaceGuard is now active and locked to your face profile.`;
      addLogEntry(`Owner face enrolled successfully (${Number(conf).toFixed(1)}% conf)`);
      pollTelemetry();
    } else {
      const msg = res?.message || res?.error || (typeof res === 'string' ? res : 'Could not detect face. Ensure camera is facing you with good lighting.');
      out.textContent = `[FAILED] ${msg}`;
      addLogEntry(`Face enrollment failed: ${msg}`);
    }
  } catch (err) {
    out.textContent = `[ERROR] Exception during enrollment: ${err}`;
  }
});

document.getElementById('btnTestRecognition').addEventListener('click', async () => {
  const out = document.getElementById('recOutput');
  out.textContent = 'Analyzing camera frame for facial identity match...';
  try {
    const res = await invoke('test_recognition');
    console.log('test_recognition response:', res);
    if (res && res.success) {
      const sim = res.similarity_percent !== undefined ? res.similarity_percent : (res.similarityPercent !== undefined ? res.similarityPercent : 0.0);
      const thresh = res.threshold_percent !== undefined ? res.threshold_percent : (res.thresholdPercent !== undefined ? res.thresholdPercent : 60.0);
      const isRec = res.is_recognized !== undefined ? res.is_recognized : res.isRecognized;
      const isEnr = res.is_enrolled !== undefined ? res.is_enrolled : res.isEnrolled;
      out.textContent = `[RECOGNITION TEST RESULT]\n` +
        `Enrolled Profile: ${isEnr ? 'YES' : 'NO'}\n` +
        `Similarity Score: ${Number(sim).toFixed(1)}%\n` +
        `Match Threshold: ${Number(thresh).toFixed(1)}%\n` +
        `Verdict: ${isRec ? 'AUTHORIZED OWNER (Match)' : 'STRANGER / UNAUTHORIZED (Mismatch)'}\n` +
        `Message: ${res.message || ''}`;
      addLogEntry(`Recognition test: match=${isRec}, sim=${Number(sim).toFixed(1)}%`);
      pollTelemetry();
    } else {
      const msg = res?.message || res?.error || 'Could not test face recognition.';
      out.textContent = `[FAILED] ${msg}`;
    }
  } catch (err) {
    out.textContent = `[ERROR] Exception during test: ${err}`;
  }
});

document.getElementById('btnClearEnrollment').addEventListener('click', async () => {
  if (!confirm('Are you sure you want to clear your enrolled face profile? The system will revert to general presence mode.')) {
    return;
  }
  try {
    await invoke('clear_enrolled_face');
    document.getElementById('recOutput').textContent = 'Enrolled face profile cleared. System reverted to general presence detection.';
    addLogEntry('Face profile cleared by user');
    pollTelemetry();
  } catch (err) {
    alert(`Failed to clear face profile: ${err}`);
  }
});

// Settings Management
async function loadSettings() {
  try {
    const cfg = await invoke('get_config');
    currentConfig = cfg;

    document.getElementById('settingInterval').value = cfg.detection_interval_seconds;
    document.getElementById('settingThreshold').value = cfg.missing_face_threshold;
    document.getElementById('settingConfidence').value = cfg.confidence_threshold;
    document.getElementById('settingLockEnabled').checked = cfg.lock_on_missing_face;
    document.getElementById('settingRecognitionEnabled').checked = cfg.recognition_enabled;
    document.getElementById('settingRecognitionThreshold').value = cfg.recognition_threshold;
    document.getElementById('settingStartWithWindows').checked = cfg.start_with_windows;
    document.getElementById('settingMinimizeToTray').checked = cfg.minimize_to_tray;

    // Load available cameras
    const camSelect = document.getElementById('settingCameraSelect');
    try {
      const cams = await invoke('list_cameras');
      camSelect.innerHTML = '';
      cams.forEach(cam => {
        const opt = document.createElement('option');
        opt.value = cam.index;
        opt.textContent = `${cam.name} (#${cam.index})`;
        if (cam.index === cfg.camera_index) {
          opt.selected = true;
        }
        camSelect.appendChild(opt);
      });
    } catch (e) {
      console.warn('Could not load camera list:', e);
    }
  } catch (err) {
    console.error('Error loading settings:', err);
  }
}

document.getElementById('btnSaveSettings').addEventListener('click', async () => {
  if (!currentConfig) return;

  const updated = {
    ...currentConfig,
    detection_interval_seconds: parseInt(document.getElementById('settingInterval').value, 10) || 20,
    missing_face_threshold: parseInt(document.getElementById('settingThreshold').value, 10) || 2,
    confidence_threshold: parseFloat(document.getElementById('settingConfidence').value) || 0.70,
    camera_index: parseInt(document.getElementById('settingCameraSelect').value, 10) || 0,
    lock_on_missing_face: document.getElementById('settingLockEnabled').checked,
    recognition_enabled: document.getElementById('settingRecognitionEnabled').checked,
    recognition_threshold: parseFloat(document.getElementById('settingRecognitionThreshold').value) || 0.60,
    start_with_windows: document.getElementById('settingStartWithWindows').checked,
    minimize_to_tray: document.getElementById('settingMinimizeToTray').checked,
  };

  try {
    await invoke('save_config', { config: updated });
    currentConfig = updated;
    const saveStatus = document.getElementById('saveStatus');
    saveStatus.style.display = 'inline';
    setTimeout(() => {
      saveStatus.style.display = 'none';
    }, 2500);
    addLogEntry('Configuration settings updated and saved');
    pollTelemetry();
  } catch (err) {
    alert(`Failed to save settings: ${err}`);
  }
});

// Initialization
window.addEventListener('DOMContentLoaded', () => {
  pollTelemetry();
  setInterval(pollTelemetry, 1000);
  loadSettings();
});

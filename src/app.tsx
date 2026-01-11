import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { motion, AnimatePresence } from "framer-motion";
import { useTranslation } from "react-i18next";
import { CheckIcon } from "@heroicons/react/24/solid";
import ky from "ky";
import LanguageSelector from "./components/language-selector";
import WindowControls from "./components/window-controls";
import "./app.css";

interface DownloadProgress {
  downloaded: number;
  total: number | null;
  percentage: number;
}

interface DownloadComplete {
  path: string;
  total?: number;
}

interface ReleaseAsset {
  id: number;
  name: string;
  browserDownloadUrl: string;
}

interface LatestRelease {
  tagName: string;
  assets: ReleaseAsset[];
}

interface StatsResponse {
  usersOnline: number;
  achievementCount: number;
  totalPlayTimeInHours: number;
  gameCount: number;
  userPlayingCount: number;
  userCount: number;
  githHubStargazes: number;
  latestRelease: LatestRelease;
}

const ANIMATION_TIMINGS = {
  LOGO_FOCUS: 100,
  LOGO_MINIMIZE: 1600,
  CONTENT_VISIBLE: 2500,
} as const;

const STATS_API_URL = "https://hydra-api-us-east-1.losbroxas.org/stats";

async function fetchLatestDownloadUrl(): Promise<string> {
  try {
    const response = await ky.get(STATS_API_URL).json<StatsResponse>();
    const setupAsset = response.latestRelease.assets.find((asset) =>
      asset.name.endsWith("-setup.exe")
    );

    if (!setupAsset) {
      throw new Error("Setup executable not found in latest release assets");
    }

    return setupAsset.browserDownloadUrl;
  } catch (error) {
    if (error instanceof Error) {
      throw new Error(`Failed to fetch latest version: ${error.message}`);
    }
    throw new Error("Failed to fetch latest version: Unknown error");
  }
}

async function fetchVersion(): Promise<string | null> {
  try {
    const response = await ky.get(STATS_API_URL).json<StatsResponse>();
    return response.latestRelease.tagName;
  } catch (error) {
    console.error("Failed to fetch version:", error);
    return null;
  }
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return Math.round(bytes / Math.pow(k, i)) + " " + sizes[i];
}

function scheduleAnimation(callbacks: Array<{ time: number; callback: () => void }>) {
  const startTime = performance.now();
  let animationFrameId: number;
  const executed = new Set<number>();

  function animate(currentTime: number) {
    const elapsed = currentTime - startTime;

    callbacks.forEach(({ time, callback }) => {
      if (elapsed >= time && !executed.has(time)) {
        callback();
        executed.add(time);
      }
    });

    if (executed.size < callbacks.length) {
      animationFrameId = requestAnimationFrame(animate);
    }
  }

  animationFrameId = requestAnimationFrame(animate);
  return () => cancelAnimationFrame(animationFrameId);
}

function App() {
  const { t } = useTranslation();
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState(0);
  const [downloaded, setDownloaded] = useState(0);
  const [totalSize, setTotalSize] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [logoFocused, setLogoFocused] = useState(false);
  const [logoMinimized, setLogoMinimized] = useState(false);
  const [contentVisible, setContentVisible] = useState(false);
  const [hasPreviousInstallation, setHasPreviousInstallation] = useState(false);
  const [deletePreviousInstallation, setDeletePreviousInstallation] = useState(false);
  const [version, setVersion] = useState<string | null>(null);

  async function startDownload() {
    try {
      if (deletePreviousInstallation) {
        await invoke("delete_previous_installation");
      }

      setDownloading(true);
      setProgress(0);
      setDownloaded(0);
      setTotalSize(null);
      setError(null);

      const downloadUrl = await fetchLatestDownloadUrl();
      await invoke("start_download", { url: downloadUrl });
    } catch (err) {
      setError(err as string);
      setDownloading(false);
    }
  }

  useEffect(() => {
    async function showWindow() {
      try {
        await invoke("show_main_window");
      } catch (err) {
        console.error("Error showing main window:", err);
      }
    }
    showWindow();

    const cancelAnimation = scheduleAnimation([
      { time: ANIMATION_TIMINGS.LOGO_FOCUS, callback: () => setLogoFocused(true) },
      { time: ANIMATION_TIMINGS.LOGO_MINIMIZE, callback: () => setLogoMinimized(true) },
      { time: ANIMATION_TIMINGS.CONTENT_VISIBLE, callback: () => setContentVisible(true) },
    ]);

    return () => {
      cancelAnimation();
    };
  }, []);

  useEffect(() => {
    async function checkInstallation() {
      try {
        const exists = await invoke<boolean>("check_previous_installation");
        setHasPreviousInstallation(exists);
      } catch (err) {
        console.error("Error checking previous installation:", err);
      }
    }
    checkInstallation();
  }, []);

  useEffect(() => {
    async function loadVersion() {
      const versionTag = await fetchVersion();
      if (versionTag) {
        const versionNumber = versionTag.startsWith("v") ? versionTag.slice(1) : versionTag;
        setVersion(versionNumber);
      }
    }
    loadVersion();
  }, []);

  useEffect(() => {
    const unlistenProgress = listen<DownloadProgress>("download-progress", (event) => {
      const progressData = event.payload;
      if (progressData.percentage >= 0) {
        setProgress(progressData.percentage);
      }
      setDownloaded(progressData.downloaded);
      if (progressData.total !== null && progressData.total !== undefined) {
        setTotalSize(progressData.total);
      }
    });

    const unlistenComplete = listen<DownloadComplete>("download-complete", async (event) => {
      try {
        await invoke("run_installer", { installerPath: event.payload.path });
        await new Promise(resolve => setTimeout(resolve, 500));
        await invoke("exit_app");
      } catch (err) {
        console.error("Failed to run installer:", err);
        setError(`Failed to run installer: ${err}`);
        setDownloading(false);
      }
    });

    const unlistenError = listen<{ error: string }>("download-error", (event) => {
      setError(event.payload.error);
      setDownloading(false);
    });

    return () => {
      unlistenProgress.then((unlisten) => unlisten());
      unlistenComplete.then((unlisten) => unlisten());
      unlistenError.then((unlisten) => unlisten());
    };
  }, []);

  return (
    <>
      <video
        className="background-video"
        autoPlay
        loop
        muted
        playsInline
      >
        <source src="/hydra-clouds-2.mp4" type="video/mp4" />
      </video>
      <AnimatePresence mode="wait">
        <motion.div
          className="logo-container logo-container-animated"
          initial={{ 
            x: '-50%',
            y: '-50%',
            opacity: 0,
            scale: 1
          }}
          animate={logoMinimized ? {
            x: '-50%',
            y: -350,
            opacity: 1,
            scale: 0.35,
            transition: {
              duration: 1.2,
              ease: [0.4, 0, 0.2, 1]
            }
          } : logoFocused ? {
            x: '-50%',
            y: '-50%',
            opacity: 1,
            scale: 1,
            transition: {
              duration: 1.5,
              ease: [0.4, 0, 0.2, 1]
            }
          } : {
            x: '-50%',
            y: '-50%',
            opacity: 0,
            scale: 1,
            transition: {
              duration: 0.3
            }
          }}
        >
          <motion.img 
            src="/hydra.svg" 
            alt="Hydra Logo" 
            className={`hydra-logo ${logoFocused ? 'hydra-logo-focused' : 'hydra-logo-blurred'}`}
          />
        </motion.div>
      </AnimatePresence>
      <main className="container">
        <div className="download-card">
          <div className="download-card-header">
            <motion.div
              className="download-card-header-content"
              initial={{ opacity: 0 }}
              animate={{ 
                opacity: contentVisible ? 1 : 0,
                transition: {
                  duration: 0.8,
                  ease: [0.4, 0, 0.2, 1]
                }
              }}
            >
              <div className="download-indicator-container">
                <div className={`step-orb ${!downloading ? 'active' : ''}`} />
                <div className={`step-orb ${downloading ? 'active' : ''}`} />
              </div>
              <h1 className="download-title">
                {downloading
                  ? t("title.downloading")
                  : t("title.default")}
              </h1>
              {version && (
                <div className="download-version">
                  Ver. {version}
                </div>
              )}
              <p className="download-description">
                {t("description")}
              </p>
            </motion.div>
          </div>

          <motion.div
            className="download-card-content"
            initial={{ opacity: 0 }}
            animate={{ 
              opacity: contentVisible ? 1 : 0,
              transition: {
                duration: 0.8,
                ease: [0.4, 0, 0.2, 1]
              }
            }}
          >
            {downloading ? (
              <div className="download-progress-section">
                <div className="download-progress-bar">
                  <div
                    className="download-progress-fill"
                    style={{ width: `${progress}%` }}
                  />
                </div>
                <div className="download-progress-info">
                  <span className="download-progress-percentage">{Math.round(progress)}%</span>
                  <span className="download-progress-size">
                    {formatBytes(downloaded)} / {totalSize ? formatBytes(totalSize) : '...'}
                  </span>
                </div>
              </div>
            ) : (
              <>
                {hasPreviousInstallation && (
                  <label className="delete-previous-checkbox">
                    <div className="custom-checkbox">
                      <input
                        type="checkbox"
                        checked={deletePreviousInstallation}
                        onChange={(e) => setDeletePreviousInstallation(e.target.checked)}
                      />
                      {deletePreviousInstallation && (
                        <div className="checkbox-check">
                          <CheckIcon />
                        </div>
                      )}
                    </div>
                    <span>{t("deletePreviousInstallation")}</span>
                  </label>
                )}
                <button
                  onClick={startDownload}
                  className="download-start-button"
                >
                  {t("startDownload")}
                </button>
              </>
            )}

            {error && (
              <p className="download-error-message">
                {t("error")}: {error}
              </p>
            )}
          </motion.div>
        </div>
      </main>
      <LanguageSelector visible={contentVisible} />
      <WindowControls visible={contentVisible} />
    </>
  );
}

export default App;

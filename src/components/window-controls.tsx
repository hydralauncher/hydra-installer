import { invoke } from "@tauri-apps/api/core";
import { motion } from "framer-motion";
import { MinusIcon, XMarkIcon } from "@heroicons/react/24/outline";
import "./window-controls.css";

interface WindowControlsProps {
  visible: boolean;
}

export default function WindowControls({ visible }: Readonly<WindowControlsProps>) {
  const handleMinimize = async () => {
    try {
      await invoke("minimize_window");
    } catch (err) {
      console.error("Error minimizing window:", err);
    }
  };

  const handleClose = async () => {
    try {
      await invoke("exit_app");
    } catch (err) {
      console.error("Error closing window:", err);
    }
  };

  return (
    <motion.div
      className="window-controls"
      initial={{ opacity: 0 }}
      animate={{
        opacity: visible ? 1 : 0,
        transition: {
          duration: 0.8,
          ease: [0.4, 0, 0.2, 1]
        }
      }}
    >
      <button
        className="window-control-button minimize-button"
        onClick={handleMinimize}
        aria-label="Minimize"
      >
        <MinusIcon className="window-control-icon" />
      </button>
      <button
        className="window-control-button close-button"
        onClick={handleClose}
        aria-label="Close"
      >
        <XMarkIcon className="window-control-icon" />
      </button>
    </motion.div>
  );
}


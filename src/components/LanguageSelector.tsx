import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { GlobeAltIcon } from "@heroicons/react/24/outline";
import "./LanguageSelector.css";

const languages = [
  { code: "en", name: "English", flag: "🇬🇧" },
  { code: "pt", name: "Português", flag: "🇵🇹" },
  { code: "ru", name: "Русский", flag: "🇷🇺" },
  { code: "es", name: "Español", flag: "🇪🇸" },
];

interface LanguageSelectorProps {
  visible: boolean;
}

export default function LanguageSelector({ visible }: LanguageSelectorProps) {
  const { i18n, t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const currentLang = languages.find((lang) => lang.code === i18n.language) || languages[0];
  const dropdownRef = useRef<HTMLDivElement>(null);

  const changeLanguage = (langCode: string) => {
    i18n.changeLanguage(langCode);
    setIsOpen(false);
  };

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [isOpen]);

  return (
    <motion.div
      className="language-selector"
      initial={{ opacity: 0 }}
      animate={{
        opacity: visible ? 1 : 0,
        transition: {
          duration: 0.8,
          ease: [0.4, 0, 0.2, 1]
        }
      }}
      ref={dropdownRef}
    >
      <button
        className="language-selector-button"
        onClick={() => setIsOpen(!isOpen)}
        aria-label={t("selectLanguage")}
      >
        <GlobeAltIcon className="language-globe" />
        <span className="language-name-display">{currentLang.name}</span>
      </button>
      <AnimatePresence>
        {isOpen && (
          <motion.div
            className="language-dropdown"
            initial={{ opacity: 0, y: 10, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 10, scale: 0.95 }}
            transition={{
              duration: 0.2,
              ease: [0.4, 0, 0.2, 1]
            }}
          >
            {languages.map((lang) => (
              <button
                key={lang.code}
                className={`language-option ${i18n.language === lang.code ? "active" : ""}`}
                onClick={() => changeLanguage(lang.code)}
              >
                <span className="language-name">{lang.name}</span>
              </button>
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
}


import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from './locales/en.json';
import pt from './locales/pt.json';
import ru from './locales/ru.json';
import es from './locales/es.json';

const getBrowserLanguage = (): string => {
  const lang = navigator.language || (navigator as any).userLanguage;
  const langCode = lang.split('-')[0].toLowerCase();
  
  const supportedLanguages = ['en', 'pt', 'ru', 'es'];
  if (supportedLanguages.includes(langCode)) {
    return langCode;
  }
  
  return 'en';
};

i18n
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      pt: { translation: pt },
      ru: { translation: ru },
      es: { translation: es },
    },
    lng: getBrowserLanguage(),
    fallbackLng: 'en',
    interpolation: {
      escapeValue: false,
    },
  });

export default i18n;


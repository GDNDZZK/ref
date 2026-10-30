import { createPinia } from 'pinia'
import { createApp } from 'vue'
import App from './App.vue'
import { installLanguage } from './composables/useLanguage.ts'
import { installTheme } from './composables/useTheme.ts'
import { createAppI18n } from './i18n/index.ts'
import { boot } from './stores/boot.ts'
import { useConnectionStore } from './stores/connection.ts'
import './theme/generated/theme.css'
import './styles/base.css'

const app = createApp(App).use(createPinia())
const i18n = createAppI18n()
installTheme()
installLanguage(i18n)
app.use(i18n).mount('#app')

void boot().then((error) => {
  if (error) useConnectionStore().fatal = error.messageKey
})

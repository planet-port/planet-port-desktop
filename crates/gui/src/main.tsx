import ReactDOM from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import App from './App';
import { ToastProvider } from './components/Toaster';
import './i18n';
import { initTheme } from './theme';
import { initBackground } from './background';
initTheme();
initBackground();
import './styles.css';
import '../tailwind.css';

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <QueryClientProvider client={queryClient}>
    <ToastProvider>
      <App />
    </ToastProvider>
  </QueryClientProvider>,
);

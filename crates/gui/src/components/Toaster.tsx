import { createContext, useCallback, useContext, useRef, useState } from 'react';

interface Toast {
  id: number;
  kind: 'ok' | 'error';
  text: string;
}

const Ctx = createContext<{ ok: (t: string) => void; error: (t: string) => void }>({ ok: () => {}, error: () => {} });

export const useToast = () => useContext(Ctx);

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [items, setItems] = useState<Toast[]>([]);
  const seq = useRef(0);
  const push = useCallback((kind: 'ok' | 'error', text: string) => {
    const id = ++seq.current;
    setItems(v => [...v, { id, kind, text }]);
    setTimeout(() => setItems(v => v.filter(i => i.id !== id)), 4000);
  }, []);
  return (
    <Ctx.Provider value={{ ok: t => push('ok', t), error: t => push('error', t) }}>
      {children}
      <div className="toasts">
        {items.map(i => (
          <div key={i.id} className={`toast ${i.kind}`}>
            {i.text}
          </div>
        ))}
      </div>
    </Ctx.Provider>
  );
}

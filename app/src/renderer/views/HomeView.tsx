import { FolderOpen } from 'lucide-react'
import { usePlayer } from '../stores/player'
import s from './HomeView.module.css'

export function HomeView() {
  const queue = usePlayer((p) => p.queue)
  const current = usePlayer((p) => p.state.queueIndex)
  const { openFiles, playItem, playPaths, api } = usePlayer.getState()
  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    const paths = api?.pathsForFiles([...e.dataTransfer.files]) ?? []
    if (paths.length) void playPaths(paths)
  }
  return (
    <div className={s.home} onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <header className={s.head}>
        <div>
          <div className="mono muted">QUEUE · {queue.items.length} {queue.items.length === 1 ? 'TRACK' : 'TRACKS'}</div>
          <h1>{queue.items.length ? 'Now playing' : 'Good to hear you'}</h1>
        </div>
        <button className={s.open} onClick={() => void openFiles()}><FolderOpen size={16} /> Open files</button>
      </header>
      {queue.items.length === 0 ? (
        <div className={s.empty}>
          <p>Drop audio files here, or open them to start listening.</p>
          <p className="mono muted">FLAC · ALAC · WAV · AIFF · MP3 · AAC · HE-AAC · Vorbis · Opus · DSF · DFF</p>
        </div>
      ) : (
        <ol className={s.list}>
          {queue.items.map((it, i) => (
            <li key={it.id} className={i === current ? s.current : undefined}>
              <button onDoubleClick={() => void playItem(it.id)} onKeyDown={(e) => { if (e.key === 'Enter') void playItem(it.id) }}>
                <span className="mono muted">{String(i + 1).padStart(2, '0')}</span>
                <span className={s.name}>{it.title}</span>
                <span className="mono muted">{it.path.split(/[\\/]/).slice(-2, -1)[0]}</span>
              </button>
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}

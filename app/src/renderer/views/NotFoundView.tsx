import { Link } from '@tanstack/react-router'
import { S } from '../lib/strings'

export function NotFoundView() {
  return <div style={{ padding: 40 }}><p>{S.notFound}</p><Link to="/" style={{ color: 'var(--accent)' }}>{S.goHome}</Link></div>
}

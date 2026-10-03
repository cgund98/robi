import type { ReviewLine } from '../../api/review'
import type { PaintedToken } from './highlight'
import styles from './DiffList.module.css'

type DiffRowsProps = {
  lines: ReviewLine[]
  tokensForLine: (line: ReviewLine) => PaintedToken[]
}

/** Gutter and code are separate columns so a copy takes the code only. */
export function DiffRows({ lines, tokensForLine }: DiffRowsProps) {
  return (
    <div className={styles.sheet}>
      <div className={styles.gutter} aria-hidden="true">
        {lines.map((line, index) => (
          <div key={index} className={styles.gutterLine} data-kind={line.kind}>
            <span className={styles.num}>{line.old_line ?? ''}</span>
            <span className={styles.num}>{line.new_line ?? ''}</span>
            <span className={styles.mark}>
              {line.kind === 'delete' ? '−' : line.kind === 'insert' ? '+' : ' '}
            </span>
          </div>
        ))}
      </div>
      <div className={styles.codeCol}>
        {lines.map((line, index) => {
          const tokens = tokensForLine(line)
          const empty = tokens.every((token) => token.text === '')
          return (
            <div key={index} className={styles.codeLine} data-kind={line.kind}>
              {empty
                ? '\u00a0'
                : tokens.map((token, tokenIndex) => (
                    <span key={tokenIndex} style={token.color ? { color: token.color } : undefined}>
                      {token.text}
                    </span>
                  ))}
            </div>
          )
        })}
      </div>
    </div>
  )
}

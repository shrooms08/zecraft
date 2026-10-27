export function Footer({ githubUrl }: { githubUrl: string }) {
  return (
    <footer className="footer">
      <p>Built with Zecraft: an open-source kit for fair, private games on Zcash.</p>
      <p>
        <a href={githubUrl}>GitHub</a>
      </p>
    </footer>
  );
}

  ---
  Critique

  C3. secure(false) hardcodé sur les cookies (auth.rs:280). Le commentaire promet "flipped to true behind TLS reverse proxy via config later" — ça n'est jamais branché. Si jamais Iris est servi en HTTP brut, l'attaquant capture les cookies via Wi-Fi/proxy. Fix : driver depuis
  cfg().server (ou détecter X-Forwarded-Proto).

  Élevé

  E1. CORS allow_origin(Any) + allow_headers(Any) (app.rs:38-42). SameSite=Lax sauve l'auth par cookies (browser ne mettra pas Access-Control-Allow-Credentials sans origin explicite), mais un Bearer token volé peut être POST depuis n'importe quel origin. Restreindre à une liste d'origins
   de config.

  E2. DELETE /torrents/{infohash} ouvert à tout user authentifié (routes/torrents.rs:562-564). Idem ingest. Étant donné le modèle "invitation-only multi-user", tout invité peut détruire la bibliothèque d'autrui. À discuter : owner-or-admin (added_by == user.id || is_admin) plutôt que
  any.

  Moyen

  M3. Pas de revocation pour les access tokens. Argon2 + invalidation de refresh sur change-password : OK. Mais l'access token survit jusqu'à exp (1h) — un user dégradé d'admin reste admin pendant 1h. Compromis classique JWT ; documenter, ou raccourcir access_ttl_secs sur les routes
  admin via une vérif DB additionnelle.

  M4. Cookie de logout ne re-set pas explicitement secure/samesite/httponly sur la version supprimée (auth.rs:201-202). Sur certains browsers la suppression peut échouer si les attrs diffèrent. À durcir par symétrie avec build_cookie.

  Bas

  B1. Validation email faiblarde : juste contains('@'). Impact limité (invite-only).

  B2. Password : min 8, aucun check de complexité. Acceptable pour invite-only ; envisager zxcvbn ou min 12.

  B3. Validation::default() sur les JWT ne vérifie pas aud (on n'en a pas, donc OK) ni nbf (on n'en émet pas). On vérifie iss et kind (custom). Solid mais explicitement validation.validate_aud = false rendrait l'intent visible.

  B4. Aucun security header côté SPA (ServeDir brut). Pas de CSP, X-Frame-Options, Referrer-Policy. Cookies httpOnly protègent contre XSS-vol-de-cookie, mais ajouter tower_http::set_header durcirait l'ensemble.

  B5. Pas de rate-limit sur /auth/device/poll/{device_id}. UUID v4 = 122 bits → brute-force impraticable. Hygiène seulement.

  Bon

  ✓ Argon2id pour les mots de passe, params défaut (m=19456, t=2, p=1) — OWASP-compliant.
  ✓ Refresh token rotation (revoke old + issue new sur chaque /refresh), avec stockage jti côté DB pour la révocation par-token.
  ✓ Invitations stockées en hash (hash_invitation_token).
  ✓ is_safe_asset_name solide pour le HLS cache (alphanumeric + _-., len<128, no ..).
  ✓ Toutes les routes torrent/library/search/me/admin sont gardées par AuthUser/AdminUser. /health est le seul endpoint public.
  ✓ sqlx paramétrisé partout — pas d'injection SQL trouvée.
  ✓ JWT secret obligatoire en config (pas de default).
  ✓ Continue-watching / progress scopés par user.id — pas de leak cross-user.
  ✓ Code de pairing : alphabet anti-confusion, TTL 10min, 31⁸≈8.5e11 espace, hashage non requis (TTL court).
  ✓ Path traversal sur play_asset impossible : l'infohash doit exister en DB avant que la key soit construite, et is_safe_asset_name filtre le segment d'asset.

  Priorité d'action : C1 et C2 d'abord (simples + impact direct), C3 ensuite quand tu confirmes le mode de déploiement (TLS terminé où ?), puis E2.
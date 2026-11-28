-- Show the readiness card again (2026-11-28).
--
-- Before Nimrag had its own readiness estimate, the readiness card started hidden because the
-- Instinct 2 writes no readiness of its own. Layouts saved in that time still list it as hidden,
-- so owners never see the estimate. Take it out of `hidden` once, and move it right after the
-- Energy card (where the default order has it). Layouts that are not the expected JSON shape are
-- left alone.
UPDATE user_settings
SET value = json_set(
    value,
    '$.hidden',
    json((
        SELECT json_group_array(h.value)
        FROM json_each(user_settings.value, '$.hidden') h
        WHERE h.value <> 'training_readiness'
    )),
    '$.order',
    json((
        SELECT json_group_array(v)
        FROM (
            SELECT o.value AS v,
                   CASE
                       WHEN o.value = 'training_readiness' THEN COALESCE((
                           SELECT e.key FROM json_each(user_settings.value, '$.order') e
                           WHERE e.value = 'energy'
                       ), -1) + 0.5
                       ELSE o.key
                   END AS pos
            FROM json_each(user_settings.value, '$.order') o
            ORDER BY pos
        )
    ))
)
WHERE key = 'home_cards'
  AND json_valid(value)
  AND json_type(value, '$.hidden') = 'array'
  AND json_type(value, '$.order') = 'array'
  AND EXISTS (
      SELECT 1 FROM json_each(user_settings.value, '$.hidden') h
      WHERE h.value = 'training_readiness'
  );

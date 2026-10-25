-- Sandbox level scripts. Each global table is an entity class: the
-- engine makes one instance per entity whose `script` names it, and
-- calls its on_* methods (on_spawn, on_fire, on_use, on_enter,
-- on_tick(dt)). `self` is the instance: store state on it.
--
-- Talk to the engine with world.fire(name), world.fire_after(seconds,
-- name) and world.log(text). Save this file while the game runs to
-- reload it: classes update, entities keep their state.

-- The button on the room's back wall. Three presses open the secret
-- panel in the west wall; after that it does nothing.
SecretButton = {}

local PRESSES_NEEDED = 3

function SecretButton:on_use()
    if self.opened then
        world.log("The button clicks. Nothing else happens.")
        return
    end

    self.presses = (self.presses or 0) + 1
    local left = PRESSES_NEEDED - self.presses
    if left > 0 then
        world.log(("Click. %d more."):format(left))
        return
    end

    self.opened = true
    world.log("A grinding sound from the west wall.")
    world.fire("secret")
end

-- Sandbox level scripts. Each global table is an entity class: the
-- engine makes one instance per entity whose `script` names it, and
-- calls its on_* methods (on_spawn, on_fire, on_use, on_enter,
-- on_tick(dt)). `self` is the instance: store state on it.
--
-- Act with world.fire(name), world.fire_after(seconds, name),
-- world.open(target), world.close(target) and world.log(text). Read
-- with world.position(target), world.mover(target), world.player() and
-- world.time(). A target is a name, or `self`. Save this file while the
-- game runs to reload it: classes update, entities keep their state.

-- The button on the room's back wall. Three presses open the secret
-- panel in the west wall; once found, one press reopens it. The panel
-- shuts again when the player wanders off.
SecretButton = {}

local PRESSES_NEEDED = 3
-- Meters from the panel at which it closes behind the player.
local CLOSE_DISTANCE = 6

-- Ground distance between two {x, y, z} points.
local function distance(a, b)
    local dx, dz = a.x - b.x, a.z - b.z
    return math.sqrt(dx * dx + dz * dz)
end

function SecretButton:on_use()
    if self.found then
        world.open("secret")
        world.log("The panel grinds open again.")
        return
    end

    self.presses = (self.presses or 0) + 1
    local left = PRESSES_NEEDED - self.presses
    if left > 0 then
        world.log(("Click. %d more."):format(left))
        return
    end

    self.found = true
    world.log("A grinding sound from the west wall.")
    world.open("secret")
end

function SecretButton:on_tick()
    local panel = world.mover("secret")
    local player = world.player()
    if not panel or panel.state ~= "open" or not player then
        return
    end

    if distance(player, world.position("secret")) > CLOSE_DISTANCE then
        world.close("secret")
        world.log("The panel slides shut behind you.")
    end
end
